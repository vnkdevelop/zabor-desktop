#![cfg(feature = "native-media")]

use futures_util::StreamExt;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::peer_connection_factory::{native::PeerConnectionFactoryExt, PeerConnectionFactory};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_mix::{AudioControls, AudioMixer};
use zabor_native::audio_runtime::{
    AudioDemand, AudioFlow, CaptureInput, ProcessorRetry, SharedAudio, SharedAudioDevice,
};
use zabor_native::audio_settings::AudioSettings;
use zabor_native::mic_test::{
    MicTest, MicTestFailure, MicTestIssue, MicTestStage, CLIP_SAMPLES, MONITOR_SETTLE,
    PIPELINE_FLUSH,
};
use zabor_native::mic_test_ui::MicTestUi;
use zabor_native::microphone::{audio_source, MicProcessor, MicSubmission};

fn settings() -> AudioSettings {
    AudioSettings {
        noise_suppression: false,
        echo_cancellation: false,
        ..AudioSettings::default()
    }
}

fn clip(test: &MicTest, sample: f32) -> Instant {
    let now = Instant::now();
    let token = test.begin(now).unwrap();
    assert!(test.prepared(token, now));
    test.snapshot(now + MONITOR_SETTLE);
    for index in 0..500 {
        assert!(test.capture(
            token,
            &[sample; 480],
            -20.0,
            now + MONITOR_SETTLE + PIPELINE_FLUSH + Duration::from_millis(index * 10)
        ));
    }
    now + Duration::from_secs(6)
}

struct Device {
    stop: CancellationToken,
    processor_failed: AtomicBool,
    retry: ProcessorRetry,
}

impl SharedAudioDevice for Device {
    fn failed(&self) -> bool {
        self.runtime_failed() || self.processor_failed.load(Ordering::Acquire)
    }
    fn cancel(&self) {
        self.retry.cancel();
    }
    fn runtime_failed(&self) -> bool {
        self.stop.is_cancelled()
    }
    fn retry_processor(&self) -> Option<watch::Receiver<Option<bool>>> {
        self.retry.request()
    }
    fn processor_recovering(&self) -> bool {
        self.retry.recovering()
    }
}

fn shared() -> (
    Arc<SharedAudio<Device>>,
    Arc<AtomicUsize>,
    CancellationToken,
) {
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    let stop = CancellationToken::new();
    let shared = SharedAudio::new(
        Arc::new(AudioControls::new(settings())),
        stop.clone(),
        move |_, _, stop| {
            counted.fetch_add(1, Ordering::AcqRel);
            Box::pin(async move {
                Ok(Device {
                    retry: ProcessorRetry::new(stop.clone()),
                    stop,
                    processor_failed: AtomicBool::new(false),
                })
            })
        },
    );
    (shared, starts, stop)
}

async fn settle(ui: &mut MicTestUi<Device>, now: Instant, stage: MicTestStage) {
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = ui.snapshot(now);
            if snapshot.stage == stage && snapshot.issue != Some(MicTestIssue::Failed) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn idle_processor_failure_revokes_only_old_input_frames_and_resets_the_meter() {
    let test = MicTest::default();
    test.configure(true, true);
    let now = Instant::now();
    let old = test.capture_token().unwrap();
    assert!(test.capture(old, &[0.1; 480], -20.0, now));
    test.fail(old, MicTestFailure::Processor);
    let failed = test.snapshot(now);
    assert_eq!(failed.meter_db, -100.0);
    assert_eq!(failed.failure, Some(MicTestFailure::Processor));
    assert_eq!(failed.stage, MicTestStage::Idle);
    assert!(!test.capture(old, &[0.2; 480], -10.0, now));
    let current = test.capture_token().unwrap();
    assert_ne!(old, current);
    test.fail(current, MicTestFailure::Processor);
    assert_eq!(test.capture_token(), Some(current));
    assert!(test.capture(current, &[0.1; 480], -22.0, now));
    assert_eq!(test.snapshot(now).issue, None);
    assert_eq!(test.snapshot(now).meter_db, -22.0);
    let recording = test.begin(now).unwrap();
    test.fail(current, MicTestFailure::Processor);
    assert_eq!(test.snapshot(now).stage, MicTestStage::Arming);
    test.fail(recording, MicTestFailure::Processor);
    assert_eq!(test.snapshot(now).stage, MicTestStage::Idle);
    test.dispose();
    test.fail(
        test.capture_token().unwrap_or(old),
        MicTestFailure::Processor,
    );
    assert_eq!(test.snapshot(now), Default::default());
}

#[test]
fn input_failure_preserves_a_silent_clip_position_and_successful_write_ticket() {
    let test = MicTest::default();
    test.configure(true, true);
    let now = clip(&test, 0.0);
    test.seek(1.5);
    let old = test.capture_token().unwrap();
    let mut output = [0.0; 960];
    let ticket = test.render(&mut output, 7, 1.0).unwrap();
    test.fail(old, MicTestFailure::Processor);
    let failed = test.snapshot(now);
    assert_eq!(failed.stored_samples, CLIP_SAMPLES);
    assert_eq!(failed.stage, MicTestStage::Ready);
    assert!(failed.playing);
    assert_eq!(failed.position_seconds, 1.5);
    assert_eq!(failed.issue, Some(MicTestIssue::Failed));
    assert!(test.validate_playback(ticket, 7));
    assert!(test.played(ticket, 7));
    assert!(!test.capture(old, &[0.1; 480], -20.0, now));
    assert!(test.capture(test.capture_token().unwrap(), &[0.1; 480], -20.0, now));
    assert_eq!(test.snapshot(now).issue, Some(MicTestIssue::Silent));
    assert_eq!(test.snapshot(now).position_seconds, 1.51);
}

#[tokio::test]
async fn shared_processor_failure_preserves_voice_device_and_output_without_a_background_retry() {
    let (shared, starts, stop) = shared();
    let monitor = shared.monitor(true);
    let device = monitor.ready().await.unwrap();
    let voice = shared.voice().await.unwrap();
    assert!(voice.activate());
    let generation = shared.demand.active();
    let now = clip(&shared.mic_test, 0.1);
    device.processor_failed.store(true, Ordering::Release);
    shared.maintain(now);
    assert!(monitor.failed());
    assert!(voice.failed());
    assert!(monitor.playback_available());
    assert_eq!(shared.mic_test.snapshot(now).meter_db, -100.0);
    assert_eq!(shared.mic_test.snapshot(now).stored_samples, CLIP_SAMPLES);
    let supervisor = tokio::spawn(shared.clone().run());
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(!device.retry.recovering());
    assert_eq!(starts.load(Ordering::Acquire), 1);
    let one = monitor.ready();
    let two = monitor.ready();
    let ready = device.retry.take().unwrap();
    assert!(device.retry.take().is_none());
    device.processor_failed.store(false, Ordering::Release);
    ready.send_replace(Some(true));
    let (one, two) = tokio::join!(one, two);
    assert!(Arc::ptr_eq(&one.unwrap(), &device));
    assert!(Arc::ptr_eq(&two.unwrap(), voice.device().unwrap()));
    assert_eq!(generation, shared.demand.active());
    assert_eq!(starts.load(Ordering::Acquire), 1);
    assert!(!voice.failed());
    stop.cancel();
    supervisor.await.unwrap();
}

#[tokio::test]
async fn a_completed_failed_retry_is_reported_once_and_requires_another_explicit_request() {
    let (shared, starts, _) = shared();
    let monitor = shared.monitor(true);
    let device = monitor.ready().await.unwrap();
    device.processor_failed.store(true, Ordering::Release);
    let failed = monitor.ready();
    device.retry.take().unwrap().send_replace(Some(false));
    assert!(failed.await.is_err());
    assert!(device.retry.take().is_none());
    shared.maintain(Instant::now());
    assert!(!device.stop.is_cancelled());
    let retry = monitor.ready();
    device.processor_failed.store(false, Ordering::Release);
    device.retry.take().unwrap().send_replace(Some(true));
    assert!(Arc::ptr_eq(&retry.await.unwrap(), &device));
    assert_eq!(starts.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn dropped_recovery_waiters_do_not_start_another_attempt_while_owner_work_is_inflight() {
    let (shared, starts, _) = shared();
    let old = shared.monitor(true);
    let device = old.ready().await.unwrap();
    device.processor_failed.store(true, Ordering::Release);
    let first = old.ready();
    let work = device.retry.take().unwrap();
    drop(first);
    drop(old);
    let current = shared.monitor(true);
    let next = current.ready();
    assert!(device.retry.take().is_none());
    device.processor_failed.store(false, Ordering::Release);
    work.send_replace(Some(true));
    assert!(Arc::ptr_eq(&next.await.unwrap(), &device));
    assert_eq!(starts.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn reset_during_recovery_rejects_old_leases_and_never_reuses_their_recovery_ticket() {
    let (shared, starts, stop) = shared();
    let old = shared.monitor(true);
    let device = old.ready().await.unwrap();
    device.processor_failed.store(true, Ordering::Release);
    let waiting = old.ready();
    let work = device.retry.take().unwrap();
    shared.reset();
    assert!(waiting.await.is_err());
    let current = shared.monitor(true);
    let fresh = current.ready().await.unwrap();
    device.processor_failed.store(false, Ordering::Release);
    work.send_replace(Some(true));
    assert!(old.ready().await.is_err());
    assert!(device.retry.request().is_none());
    assert!(!Arc::ptr_eq(&fresh, &device));
    assert_eq!(starts.load(Ordering::Acquire), 2);
    stop.cancel();
}

#[tokio::test]
async fn local_ui_keeps_valid_playback_on_input_failure_and_retakes_with_one_explicit_recovery() {
    let (shared, starts, _) = shared();
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, &runtime);
    let voice = shared.voice().await.unwrap();
    let device = voice.device().unwrap().clone();
    settle(&mut ui, Instant::now(), MicTestStage::Idle).await;
    let now = clip(&shared.mic_test, 0.1);
    device.processor_failed.store(true, Ordering::Release);
    shared.maintain(now);
    let failed = ui.snapshot(now);
    assert!(failed.playing);
    assert_eq!(failed.meter_db, -100.0);
    assert_eq!(failed.stored_samples, CLIP_SAMPLES);
    assert_eq!(failed.issue, Some(MicTestIssue::Failed));
    assert!(ui.toggle_playback());
    assert!(ui.seek(1.5));
    assert!(ui.toggle_playback());
    ui.synchronize("owner", true, true, true, &runtime);
    assert!(!device.retry.recovering());
    assert!(ui.record(now, &runtime));
    assert!(device.retry.recovering());
    shared.maintain(now);
    assert_eq!(ui.snapshot(now).stage, MicTestStage::Arming);
    assert!(!ui.record(now, &runtime));
    device.processor_failed.store(false, Ordering::Release);
    device.retry.take().unwrap().send_replace(Some(true));
    settle(&mut ui, now, MicTestStage::Arming).await;
    assert_eq!(starts.load(Ordering::Acquire), 1);
    ui.close();
}

#[tokio::test]
async fn closing_ui_while_recovery_waits_clears_capture_and_rejects_late_success() {
    let (shared, _, _) = shared();
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, &runtime);
    let voice = shared.voice().await.unwrap();
    let device = voice.device().unwrap().clone();
    settle(&mut ui, Instant::now(), MicTestStage::Idle).await;
    device.processor_failed.store(true, Ordering::Release);
    let now = Instant::now();
    assert!(ui.record(now, &runtime));
    let old = shared.mic_test.capture_token().unwrap();
    let work = device.retry.take().unwrap();
    ui.close();
    device.processor_failed.store(false, Ordering::Release);
    work.send_replace(Some(true));
    tokio::task::yield_now().await;
    assert_eq!(ui.snapshot(now).stored_samples, 0);
    assert!(!shared.mic_test.has_demand());
    assert!(!shared.mic_test.prepared(old, now));
    assert!(!ui.toggle_playback());
    assert!(!device.stop.is_cancelled());
}

#[test]
fn ui_preparation_admission_works_on_a_non_tokio_desktop_thread() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (shared, _, _) = shared();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, runtime.handle());
    runtime.block_on(settle(&mut ui, Instant::now(), MicTestStage::Idle));
    ui.close();
    shared.reset();
}

#[tokio::test]
async fn shared_recovery_clears_ui_health_without_discarding_or_restarting_its_clip() {
    let (shared, _, _) = shared();
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, &runtime);
    let voice = shared.voice().await.unwrap();
    let device = voice.device().unwrap().clone();
    settle(&mut ui, Instant::now(), MicTestStage::Idle).await;
    let now = clip(&shared.mic_test, 0.1);
    shared.mic_test.pause();
    device.processor_failed.store(true, Ordering::Release);
    assert_eq!(ui.snapshot(now).issue, Some(MicTestIssue::Failed));
    drop(voice);
    let rejoin = shared.voice();
    tokio::pin!(rejoin);
    tokio::select! {
        _ = &mut rejoin => panic!("Recovery completed before the owning processor"),
        _ = tokio::task::yield_now() => {},
    }
    device.processor_failed.store(false, Ordering::Release);
    device.retry.take().unwrap().send_replace(Some(true));
    let next = rejoin.await.unwrap();
    let token = shared.mic_test.capture_token().unwrap();
    assert!(shared.mic_test.capture(token, &[0.1; 480], -20.0, now));
    let recovered = ui.snapshot(now);
    assert_eq!(recovered.issue, None);
    assert_eq!(recovered.stage, MicTestStage::Ready);
    assert!(!recovered.playing);
    assert_eq!(recovered.stored_samples, CLIP_SAMPLES);
    assert!(Arc::ptr_eq(next.device().unwrap(), &device));
    ui.close();
}

#[tokio::test]
async fn expired_recovery_waits_are_bounded_but_do_not_duplicate_inflight_owner_work() {
    let (shared, starts, _) = shared();
    let monitor = shared.monitor(true);
    let device = monitor.ready().await.unwrap();
    device.processor_failed.store(true, Ordering::Release);
    let wait = monitor.ready();
    let work = device.retry.take().unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(35), wait)
        .await
        .unwrap()
        .is_err());
    assert!(device.retry.recovering());
    let explicit = monitor.ready();
    assert!(device.retry.take().is_none());
    assert_eq!(starts.load(Ordering::Acquire), 1);
    device.processor_failed.store(false, Ordering::Release);
    work.send_replace(Some(true));
    assert!(Arc::ptr_eq(&explicit.await.unwrap(), &device));
}

#[tokio::test]
async fn retired_weak_device_health_remains_visible_until_explicit_record_retry() {
    let (shared, starts, _) = shared();
    let runtime = tokio::runtime::Handle::current();
    let mut ui = MicTestUi::new(shared.clone());
    ui.synchronize("owner", true, true, true, &runtime);
    let voice = shared.voice().await.unwrap();
    let device = voice.device().unwrap().clone();
    let weak = Arc::downgrade(&device);
    drop(voice);
    settle(&mut ui, Instant::now(), MicTestStage::Idle).await;
    let token = shared.mic_test.capture_token().unwrap();
    shared
        .mic_test
        .capture(token, &[0.1; 480], -20.0, Instant::now());
    device.stop.cancel();
    shared.maintain(Instant::now());
    drop(device);
    tokio::time::timeout(Duration::from_secs(2), async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let failed = ui.snapshot(Instant::now());
    assert_eq!(failed.issue, Some(MicTestIssue::Failed));
    assert_eq!(failed.meter_db, -100.0);
    assert_eq!(starts.load(Ordering::Acquire), 1);
    assert!(ui.record(Instant::now(), &runtime));
    settle(&mut ui, Instant::now(), MicTestStage::Arming).await;
    assert_eq!(starts.load(Ordering::Acquire), 2);
    ui.close();
}

#[tokio::test]
async fn owning_processor_rebuild_restores_only_current_local_frames_and_no_network_pcm() {
    let controls = Arc::new(AudioControls::new(settings()));
    controls.set_user_state(true, true);
    controls.set_server_state(true, true);
    let flow = AudioFlow {
        demand: Arc::new(AudioDemand::new(controls.clone())),
        mic_test: Arc::new(MicTest::default()),
        mixer: Arc::new(AudioMixer::new(controls.clone())),
        controls: controls.clone(),
        output_session: 0.into(),
        capture_voice: 0.into(),
        stop: CancellationToken::new(),
    };
    let source = audio_source();
    let track = PeerConnectionFactory::default().create_audio_track("local-health", source.clone());
    let mut stream = NativeAudioStream::new(track, 48000, 1);
    let submit = MicSubmission::new(source, controls.clone(), flow.stop.clone());
    let mut processor = MicProcessor::new(controls.clone(), flow.mixer.clone()).unwrap();
    flow.mic_test.configure(true, true);
    let now = Instant::now();
    let capture = |processor: &mut MicProcessor, samples: &[f32; 480], gap| {
        flow.capture(
            processor,
            &submit,
            CaptureInput {
                samples,
                channels: 1,
                gap,
            },
            now,
        )
        .unwrap();
    };
    capture(&mut processor, &[0.1; 480], false);
    assert!(flow.mic_test.snapshot(now).meter_db > -100.0);
    capture(&mut processor, &[f32::NAN; 480], false);
    assert!(controls.processor_failed.load(Ordering::Acquire));
    assert_eq!(
        flow.mic_test.snapshot(now).failure,
        Some(MicTestFailure::Processor)
    );
    for _ in 0..3 {
        capture(&mut processor, &[0.1; 480], true);
    }
    assert!(controls.processor_failed.load(Ordering::Acquire));
    assert_eq!(flow.mic_test.snapshot(now).meter_db, -100.0);
    let epoch = controls.capture_epoch.load(Ordering::Acquire);
    processor.rebuild().unwrap();
    assert!(controls.capture_epoch.load(Ordering::Acquire) > epoch);
    capture(&mut processor, &[0.1; 480], false);
    assert!(!controls.processor_failed.load(Ordering::Acquire));
    assert_eq!(flow.mic_test.snapshot(now).issue, None);
    assert!(flow.mic_test.snapshot(now).meter_db > -100.0);
    assert_eq!(controls.user_state(), (true, true));
    assert_eq!(controls.server_state(), (true, true));
    assert!(!controls.speaking.load(Ordering::Acquire));
    assert_eq!(
        f32::from_bits(controls.mic_db.load(Ordering::Acquire)),
        -100.0
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), stream.next())
            .await
            .is_err()
    );
    stream.close();
}

#[cfg(feature = "neural-dsp")]
#[test]
fn owning_neural_rebuild_discards_failed_model_state_and_local_only_preroll() {
    use zabor_native::audio_settings::{NoiseModel, ThresholdMode};
    for model in [NoiseModel::Rnnoise, NoiseModel::Deepfilter] {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let controls = Arc::new(AudioControls::new(AudioSettings {
                    noise_suppression: true,
                    echo_cancellation: false,
                    smart_noise_model: model,
                    mic_threshold_mode: ThresholdMode::Manual,
                    ..AudioSettings::default()
                }));
                let mixer = Arc::new(AudioMixer::new(controls.clone()));
                let mut processor = MicProcessor::new(controls.clone(), mixer.clone()).unwrap();
                for block in 0..80 {
                    let samples: [f32; 480] = std::array::from_fn(|index| {
                        ((block * 480 + index) as f32 * std::f32::consts::TAU * 220.0 / 48000.0)
                            .sin()
                            * 0.2
                    });
                    let frame = processor.process_shared(&samples, 1, false, true, false);
                    assert_eq!(frame.pcm, [0; 480]);
                    assert!(frame.local.is_some());
                }
                let failed = processor.process_shared(&[f32::NAN; 480], 1, false, true, false);
                assert_eq!(failed.pcm, [0; 480]);
                assert!(failed.local.is_none());
                assert!(controls.processor_failed.load(Ordering::Acquire));
                processor.rebuild().unwrap();
                let mut fresh = MicProcessor::new(controls.clone(), mixer).unwrap();
                for _ in 0..20 {
                    let actual = processor.process_shared(&[0.0; 480], 1, false, true, false);
                    let expected = fresh.process_shared(&[0.0; 480], 1, false, true, false);
                    assert_eq!(
                        actual.local, expected.local,
                        "{model:?} failed-state recovery"
                    );
                    assert_eq!(actual.pcm, [0; 480]);
                    assert!(!controls.processor_failed.load(Ordering::Acquire));
                    assert!(!controls.speaking.load(Ordering::Acquire));
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
