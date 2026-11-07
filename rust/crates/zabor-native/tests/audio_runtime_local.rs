#![cfg(feature = "native-media")]

use futures_util::StreamExt;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::peer_connection_factory::{native::PeerConnectionFactoryExt, PeerConnectionFactory};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_mix::{AudioControls, AudioMixer};
use zabor_native::audio_runtime::{
    AudioDemand, AudioFlow, CaptureInput, SharedAudio, SharedAudioDevice,
};
use zabor_native::audio_settings::AudioSettings;
use zabor_native::media::MediaEngine;
use zabor_native::mic_test::{
    MicTest, MicTestStage, CLIP_SAMPLES, IDLE_RELEASE, MONITOR_SETTLE, PIPELINE_FLUSH,
};
use zabor_native::microphone::{audio_source, MicProcessor, MicSubmission};

struct Stats {
    starts: AtomicUsize,
    inflight: AtomicUsize,
    maximum: AtomicUsize,
    started: Semaphore,
    gate: Semaphore,
    tokens: Mutex<Vec<CancellationToken>>,
}

impl Stats {
    fn new(permits: usize) -> Arc<Self> {
        Arc::new(Self {
            starts: AtomicUsize::new(0),
            inflight: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            started: Semaphore::new(0),
            gate: Semaphore::new(permits),
            tokens: Mutex::new(Vec::new()),
        })
    }

    async fn started(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.started.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}

struct FakeDevice {
    stop: CancellationToken,
    failed: AtomicBool,
}

impl SharedAudioDevice for FakeDevice {
    fn failed(&self) -> bool {
        self.stop.is_cancelled() || self.failed.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.stop.cancel();
    }
}

fn shared(stats: Arc<Stats>, stop: CancellationToken) -> Arc<SharedAudio<FakeDevice>> {
    SharedAudio::new(
        Arc::new(AudioControls::new(AudioSettings::default())),
        stop,
        move |_, _, stop| {
            let stats = stats.clone();
            Box::pin(async move {
                stats.starts.fetch_add(1, Ordering::AcqRel);
                let count = stats.inflight.fetch_add(1, Ordering::AcqRel) + 1;
                stats.maximum.fetch_max(count, Ordering::AcqRel);
                stats.tokens.lock().unwrap().push(stop.clone());
                stats.started.add_permits(1);
                stats.gate.acquire().await.unwrap().forget();
                stats.inflight.fetch_sub(1, Ordering::AcqRel);
                Ok(FakeDevice {
                    stop,
                    failed: AtomicBool::new(false),
                })
            })
        },
    )
}

#[tokio::test]
async fn voice_and_monitor_share_one_start_and_voice_teardown_keeps_the_local_runtime() {
    let stats = Stats::new(0);
    let shared = shared(stats.clone(), CancellationToken::new());
    let monitor = shared.monitor(true);
    let voice = shared.voice();
    let monitor_ready = monitor.ready();
    let release = async {
        stats.started().await;
        stats.gate.add_permits(1);
    };
    let (voice, device, _) = tokio::join!(voice, monitor_ready, release);
    let voice = voice.unwrap();
    let device = device.unwrap();
    assert!(Arc::ptr_eq(voice.device().unwrap(), &device));
    assert_eq!(stats.starts.load(Ordering::Acquire), 1);
    assert_eq!(stats.maximum.load(Ordering::Acquire), 1);
    assert!(voice.activate());
    drop(voice);
    assert_eq!(shared.demand.active(), 0);
    shared.maintain(Instant::now() + IDLE_RELEASE * 2);
    assert!(!device.failed());
    let next = shared.voice().await.unwrap();
    assert!(Arc::ptr_eq(next.device().unwrap(), &device));
    assert!(next.activate());
    drop(monitor);
    assert!(!device.failed());
    drop(next);
    shared.reset();
    assert!(device.failed());
}

#[tokio::test]
async fn idle_runtime_is_reused_until_exactly_ten_seconds_without_demand() {
    let stats = Stats::new(1);
    let shared = shared(stats.clone(), CancellationToken::new());
    let voice = shared.voice().await.unwrap();
    let device = voice.device().unwrap().clone();
    drop(voice);
    let now = Instant::now();
    shared.maintain(now + IDLE_RELEASE - Duration::from_millis(2));
    assert!(!device.failed());
    let reused = shared.voice().await.unwrap();
    assert!(Arc::ptr_eq(reused.device().unwrap(), &device));
    drop(reused);
    shared.maintain(Instant::now() + IDLE_RELEASE);
    assert!(device.failed());
    assert_eq!(stats.starts.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn hidden_recording_retains_demand_but_ready_hidden_clip_does_not_keep_devices_alive() {
    let stats = Stats::new(1);
    let shared = shared(stats, CancellationToken::new());
    let monitor = shared.monitor(true);
    let device = monitor.ready().await.unwrap();
    let now = Instant::now();
    let token = shared.mic_test.begin(now).unwrap();
    assert!(shared.mic_test.prepared(token, now));
    shared.mic_test.snapshot(now + MONITOR_SETTLE);
    monitor.configure(false);
    shared.maintain(now + Duration::from_secs(6));
    assert!(!device.failed());
    for index in 0..500 {
        assert!(shared.mic_test.capture(
            token,
            &[0.1; 480],
            -20.0,
            now + MONITOR_SETTLE + PIPELINE_FLUSH + Duration::from_millis(index * 10)
        ));
    }
    assert_eq!(
        shared.mic_test.snapshot(now + Duration::from_secs(6)).stage,
        MicTestStage::Ready
    );
    shared.maintain(now + Duration::from_secs(6));
    shared.maintain(now + Duration::from_secs(16));
    assert!(device.failed());
    assert_eq!(
        shared
            .mic_test
            .snapshot(now + Duration::from_secs(16))
            .stored_samples,
        CLIP_SAMPLES
    );
    monitor.configure(true);
    assert!(
        !shared
            .mic_test
            .snapshot(now + Duration::from_secs(16))
            .playing
    );
    shared.reset();
}

#[tokio::test]
async fn cancelled_waiter_revokes_its_voice_and_cancels_the_only_pending_start() {
    let stats = Stats::new(0);
    let shared = shared(stats.clone(), CancellationToken::new());
    let task_shared = shared.clone();
    let task = tokio::spawn(async move { task_shared.voice().await });
    stats.started().await;
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(!shared.demand.has_voice());
    assert!(stats.tokens.lock().unwrap()[0].is_cancelled());
    stats.gate.add_permits(1);
    shared.reset();
}

#[tokio::test]
async fn cancelled_start_stays_quarantined_until_its_owning_factory_returns() {
    let stats = Stats::new(0);
    let stop = CancellationToken::new();
    let shared = shared(stats.clone(), stop.clone());
    let old = Arc::new(shared.monitor(true));
    let waiting = old.clone();
    let task = tokio::spawn(async move { waiting.ready().await });
    stats.started().await;
    shared.reset();
    assert!(task.await.unwrap().is_err());
    let current = shared.monitor(true);
    assert!(current.ready().await.is_err());
    assert_eq!(stats.starts.load(Ordering::Acquire), 1);
    assert_eq!(stats.inflight.load(Ordering::Acquire), 1);
    drop(old);
    assert!(shared.mic_test.has_demand());
    let supervisor = tokio::spawn(shared.clone().run());
    stats.gate.add_permits(1);
    stats.started().await;
    assert_eq!(stats.starts.load(Ordering::Acquire), 2);
    assert_eq!(stats.maximum.load(Ordering::Acquire), 1);
    stats.gate.add_permits(1);
    let device = current.ready().await.unwrap();
    assert!(!device.failed());
    stop.cancel();
    supervisor.await.unwrap();
    assert!(device.failed());
}

#[tokio::test]
async fn old_monitor_and_voice_leases_cannot_reactivate_or_dispose_a_new_owner() {
    let stats = Stats::new(2);
    let shared = shared(stats, CancellationToken::new());
    let old_monitor = shared.monitor(true);
    let old_voice = shared.voice().await.unwrap();
    assert!(old_voice.activate());
    shared.reset();
    let current_monitor = shared.monitor(true);
    let current_voice = shared.voice().await.unwrap();
    assert!(current_voice.activate());
    let active = shared.demand.active();
    assert!(!old_voice.activate());
    assert!(old_voice.failed());
    old_monitor.configure(false);
    assert!(shared.mic_test.has_demand());
    drop(old_voice);
    drop(old_monitor);
    assert_eq!(shared.demand.active(), active);
    assert!(shared.mic_test.has_demand());
    drop(current_monitor);
    drop(current_voice);
    shared.reset();
}

#[tokio::test]
async fn duplicate_voice_acquisition_is_rejected_without_superseding_the_current_lease() {
    let stats = Stats::new(1);
    let shared = shared(stats.clone(), CancellationToken::new());
    let voice = shared.voice().await.unwrap();
    assert!(voice.activate());
    let active = shared.demand.active();
    assert!(shared.voice().await.is_err());
    assert_eq!(shared.demand.active(), active);
    assert_eq!(stats.starts.load(Ordering::Acquire), 1);
    drop(voice);
    shared.reset();
}

#[tokio::test]
async fn failed_device_is_not_replaced_by_a_background_restart_loop() {
    let stats = Stats::new(1);
    let stop = CancellationToken::new();
    let shared = shared(stats.clone(), stop.clone());
    let monitor = shared.monitor(true);
    let device = monitor.ready().await.unwrap();
    device.failed.store(true, Ordering::Release);
    shared.maintain(Instant::now());
    let supervisor = tokio::spawn(shared.clone().run());
    tokio::time::sleep(Duration::from_millis(110)).await;
    assert_eq!(stats.starts.load(Ordering::Acquire), 1);
    stop.cancel();
    supervisor.await.unwrap();
}

fn flow() -> AudioFlow {
    let controls = Arc::new(AudioControls::new(AudioSettings {
        noise_suppression: false,
        echo_cancellation: false,
        ..AudioSettings::default()
    }));
    AudioFlow {
        demand: Arc::new(AudioDemand::new(controls.clone())),
        mic_test: Arc::new(MicTest::default()),
        mixer: Arc::new(AudioMixer::new(controls.clone())),
        controls,
        output_session: 0.into(),
        capture_voice: 0.into(),
        stop: CancellationToken::new(),
    }
}

fn record(flow: &AudioFlow, sample: f32) -> Instant {
    let now = Instant::now();
    flow.mic_test.configure(true, true);
    let token = flow.mic_test.begin(now).unwrap();
    assert!(flow.mic_test.prepared(token, now));
    flow.mic_test.snapshot(now + MONITOR_SETTLE);
    for index in 0..500 {
        flow.mic_test.capture(
            token,
            &[sample; 480],
            -20.0,
            now + MONITOR_SETTLE + PIPELINE_FLUSH + Duration::from_millis(index * 10),
        );
    }
    now + Duration::from_secs(6)
}

#[tokio::test]
async fn local_capture_without_voice_never_submits_pcm_or_publishes_network_speaking() {
    let flow = flow();
    let source = audio_source();
    let track =
        PeerConnectionFactory::default().create_audio_track("local-monitor", source.clone());
    let mut output = NativeAudioStream::new(track, 48000, 1);
    let submit = MicSubmission::new(source, flow.controls.clone(), flow.stop.clone());
    let mut processor = MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
    let now = Instant::now();
    flow.mic_test.configure(true, true);
    let token = flow.mic_test.begin(now).unwrap();
    for offset in [0, 130, 420] {
        flow.capture(
            &mut processor,
            &submit,
            CaptureInput {
                samples: &[0.2; 480],
                channels: 1,
                gap: false,
            },
            now + Duration::from_millis(offset),
        )
        .unwrap();
    }
    assert_eq!(
        flow.mic_test
            .snapshot(now + Duration::from_millis(420))
            .captured_samples,
        480
    );
    assert!(flow.mic_test.snapshot(now).meter_db > -20.0);
    assert!(!flow.controls.speaking.load(Ordering::Acquire));
    assert_eq!(
        f32::from_bits(flow.controls.mic_db.load(Ordering::Acquire)),
        -100.0
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), output.next())
            .await
            .is_err()
    );
    let voice = flow.demand.reserve().unwrap();
    assert!(flow.demand.activate(voice));
    flow.capture(
        &mut processor,
        &submit,
        CaptureInput {
            samples: &[0.2; 480],
            channels: 1,
            gap: false,
        },
        now + Duration::from_millis(430),
    )
    .unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(1), output.next())
        .await
        .unwrap()
        .unwrap();
    assert!(frame.data.iter().any(|sample| *sample != 0));
    flow.controls.set_server_state(false, true);
    flow.capture(
        &mut processor,
        &submit,
        CaptureInput {
            samples: &[0.2; 480],
            channels: 1,
            gap: false,
        },
        now + Duration::from_millis(440),
    )
    .unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(1), output.next())
        .await
        .unwrap()
        .unwrap();
    assert!(frame.data.iter().all(|sample| *sample == 0));
    assert!(flow.mic_test.snapshot(now).meter_db > -20.0);
    flow.demand.revoke(voice);
    flow.capture(
        &mut processor,
        &submit,
        CaptureInput {
            samples: &[0.2; 480],
            channels: 1,
            gap: false,
        },
        now + Duration::from_millis(450),
    )
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), output.next())
            .await
            .is_err()
    );
    assert_eq!(flow.mic_test.capture_token(), Some(token));
    assert_eq!(flow.mic_test.snapshot(now).captured_samples, 4 * 480);
    output.close();
}

#[test]
fn invalid_input_routes_are_rechecked_before_local_capture_and_do_not_advance_recording() {
    let flow = flow();
    let source = audio_source();
    let submit = MicSubmission::new(source, flow.controls.clone(), flow.stop.clone());
    let mut processor = MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
    let now = Instant::now();
    flow.mic_test.configure(true, true);
    let token = flow.mic_test.begin(now).unwrap();
    flow.mic_test.prepared(token, now);
    flow.mic_test.snapshot(now + MONITOR_SETTLE);
    flow.controls.input_device_epoch.store(1, Ordering::Release);
    flow.capture(
        &mut processor,
        &submit,
        CaptureInput {
            samples: &[0.2; 480],
            channels: 1,
            gap: false,
        },
        now + Duration::from_millis(400),
    )
    .unwrap();
    assert_eq!(flow.mic_test.snapshot(now).captured_samples, 0);
    flow.controls.input_route_epoch.store(1, Ordering::Release);
    flow.controls.capture_restarting();
    flow.capture(
        &mut processor,
        &submit,
        CaptureInput {
            samples: &[0.2; 480],
            channels: 1,
            gap: true,
        },
        now + Duration::from_millis(410),
    )
    .unwrap();
    assert_eq!(flow.mic_test.snapshot(now).captured_samples, 480);
    flow.stop.cancel();
    flow.capture(
        &mut processor,
        &submit,
        CaptureInput {
            samples: &[0.2; 480],
            channels: 1,
            gap: false,
        },
        now + Duration::from_millis(420),
    )
    .unwrap();
    assert_eq!(flow.mic_test.snapshot(now).captured_samples, 480);
}

#[test]
fn local_playback_ignores_network_deafen_peer_gain_and_focus_but_uses_global_gain() {
    let flow = flow();
    let now = record(&flow, 0.2);
    flow.controls.set_user_state(false, true);
    flow.controls.set_server_state(true, true);
    flow.mixer.focus(Some("other-stream".into()));
    let mut remote = [1.0; 960];
    let mut local = [0.0; 960];
    let mut output = [0.0; 960];
    let ticket = flow
        .render(&mut remote, &mut local, &mut output, 2.0)
        .unwrap();
    assert!(remote.iter().all(|sample| *sample == 0.0));
    assert!(output.iter().all(|sample| *sample == 0.4));
    assert_eq!(flow.mic_test.snapshot(now).position_seconds, 0.0);
    assert!(flow.mixer.reverse.lock().unwrap().is_empty());
    flow.played(&output, 17, 0, Some(ticket));
    assert_eq!(flow.mic_test.snapshot(now).position_seconds, 0.01);
    assert_eq!(flow.controls.render_delay_ms.load(Ordering::Acquire), 17);
    assert!(flow
        .mixer
        .reverse
        .lock()
        .unwrap()
        .iter()
        .all(|sample| *sample == (0.4 * 32767.0) as i16));
    assert_eq!(flow.controls.user_state(), (false, true));
    assert_eq!(flow.controls.server_state(), (true, true));
}

#[tokio::test]
async fn remote_and_local_pcm_combine_once_and_only_actual_successful_writes_feed_aec() {
    let flow = flow();
    let now = record(&flow, 0.2);
    let voice = flow.demand.reserve().unwrap();
    flow.demand.activate(voice);
    let (mut engine, _) = MediaEngine::new_data_only("local".into());
    engine.authorize_peers(["peer".into()]);
    engine.offer("peer", false).await.unwrap();
    let lane = flow
        .mixer
        .add(
            "lane".into(),
            "peer",
            engine.peer_lifetime("peer").unwrap(),
            false,
        )
        .unwrap();
    lane.push(&[8192; 480]);
    let mut remote = [0.0; 960];
    let mut local = [0.0; 960];
    let mut output = [0.0; 960];
    let ticket = flow
        .render(&mut remote, &mut local, &mut output, 1.0)
        .unwrap();
    assert_eq!(output, [0.45; 960]);
    assert!(flow.mixer.reverse.lock().unwrap().is_empty());
    assert_eq!(flow.mic_test.snapshot(now).position_seconds, 0.0);
    flow.played(&output, 10, 0, Some(ticket));
    assert_eq!(flow.mic_test.snapshot(now).position_seconds, 0.01);
    assert_eq!(flow.mixer.reverse.lock().unwrap().len(), 480);
    assert!(flow
        .mixer
        .reverse
        .lock()
        .unwrap()
        .iter()
        .all(|sample| *sample == (0.45 * 32767.0) as i16));
    flow.demand.revoke(voice);
    lane.push(&[8192; 480]);
    flow.render(&mut remote, &mut local, &mut output, 1.0)
        .unwrap();
    assert_eq!(output, [0.2; 960]);
}

#[test]
fn output_route_changes_failed_writes_and_revoked_playback_cannot_commit_old_cursor_or_aec() {
    let flow = flow();
    let now = record(&flow, 0.2);
    let mut remote = [0.0; 960];
    let mut local = [0.0; 960];
    let mut output = [0.0; 960];
    let ticket = flow
        .render(&mut remote, &mut local, &mut output, 1.0)
        .unwrap();
    flow.output_session.fetch_add(1, Ordering::AcqRel);
    flow.played(&output, 20, 0, Some(ticket));
    assert_eq!(flow.mic_test.snapshot(now).position_seconds, 0.0);
    assert!(flow.mixer.reverse.lock().unwrap().is_empty());
    let old = flow
        .render(&mut remote, &mut local, &mut output, 1.0)
        .unwrap();
    let current = flow
        .render(&mut remote, &mut local, &mut output, 1.0)
        .unwrap();
    assert!(!flow.mic_test.validate_playback(old, 1));
    flow.controls
        .output_device_epoch
        .store(1, Ordering::Release);
    assert!(flow
        .render(&mut remote, &mut local, &mut output, 1.0)
        .is_none());
    assert_eq!(output, [0.0; 960]);
    flow.played(&output, 20, 1, Some(current));
    assert_eq!(flow.mic_test.snapshot(now).position_seconds, 0.0);
    flow.controls.output_route_epoch.store(1, Ordering::Release);
    let ticket = flow
        .render(&mut remote, &mut local, &mut output, 1.0)
        .unwrap();
    flow.mic_test.pause();
    assert!(!flow.mic_test.validate_playback(ticket, 1));
    flow.played(&output, 20, 1, Some(ticket));
    assert_eq!(flow.mic_test.snapshot(now).position_seconds, 0.0);
    assert_eq!(flow.mic_test.snapshot(now).stored_samples, CLIP_SAMPLES);
}

#[cfg(feature = "neural-dsp")]
#[test]
fn voice_activation_resets_local_only_neural_preroll_before_any_network_pcm() {
    use zabor_native::audio_settings::{NoiseModel, ThresholdMode};
    for model in [NoiseModel::Rnnoise, NoiseModel::Deepfilter] {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let flow = flow();
                flow.controls.apply_settings(AudioSettings {
                    noise_suppression: true,
                    smart_noise_model: model,
                    mic_threshold_mode: ThresholdMode::Manual,
                    ..AudioSettings::default()
                });
                let mut processor =
                    MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
                let mut fresh =
                    MicProcessor::new(flow.controls.clone(), flow.mixer.clone()).unwrap();
                for block in 0..80 {
                    let samples: [f32; 480] = std::array::from_fn(|index| {
                        ((block * 480 + index) as f32 * std::f32::consts::TAU * 220.0 / 48000.0)
                            .sin()
                            * 0.2
                    });
                    let frame = processor.process_shared(&samples, 1, false, true, false);
                    assert_eq!(frame.pcm, [0; 480]);
                    assert!(frame.local.is_some());
                    assert!(!flow.controls.speaking.load(Ordering::Acquire));
                }
                let generation = flow.demand.reserve().unwrap();
                flow.demand.activate(generation);
                for _ in 0..20 {
                    let actual = processor.process_shared(&[0.0; 480], 1, false, true, true);
                    assert_eq!(actual.pcm, fresh.process(&[0.0; 480], 1, false));
                }
                let source = audio_source();
                let track = PeerConnectionFactory::default()
                    .create_audio_track("activation-race", source.clone());
                let mut stream = NativeAudioStream::new(track, 48000, 1);
                let submit = MicSubmission::new(source, flow.controls.clone(), flow.stop.clone());
                flow.demand.revoke(generation);
                let next_voice = flow.demand.reserve().unwrap();
                flow.demand.activate(next_voice);
                let late_local: [f32; 480] = std::array::from_fn(|index| {
                    (index as f32 * std::f32::consts::TAU * 440.0 / 48000.0).sin() * 0.8
                });
                let frame = processor.process_shared(&late_local, 1, false, true, false);
                assert_eq!(frame.pcm, [0; 480]);
                assert_eq!(
                    processor.epoch(),
                    flow.controls.capture_epoch.load(Ordering::Acquire)
                );
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                for index in 0..20 {
                    flow.capture(
                        &mut processor,
                        &submit,
                        CaptureInput {
                            samples: &[0.0; 480],
                            channels: 1,
                            gap: false,
                        },
                        Instant::now(),
                    )
                    .unwrap();
                    let actual = runtime.block_on(async {
                        tokio::time::timeout(Duration::from_secs(1), stream.next())
                            .await
                            .unwrap()
                            .unwrap()
                    });
                    let expected = fresh.process(&[0.0; 480], 1, false);
                    assert_eq!(
                        actual.data.as_ref(),
                        &expected,
                        "late local-only preroll, {model:?}, frame {index}"
                    );
                }
                stream.close();
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
