use crate::audio_mix::{AudioLane, AudioMixer};
use crate::audio_runtime::{AudioCleanup, AudioCleanupError};
use crate::media::{MediaError, PeerLifetime};
use futures_util::StreamExt;
use libwebrtc::audio_stream::native::{NativeAudioStream, NativeAudioStreamOptions};
use libwebrtc::audio_track::RtcAudioTrack;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

struct Subscription {
    id: String,
    lane: Arc<AudioLane>,
    mixer: Arc<AudioMixer>,
    stop: CancellationToken,
    task: tokio::task::AbortHandle,
    stream: bool,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop.cancel();
        self.mixer.remove_lane(&self.id, &self.lane);
        self.task.abort();
    }
}

pub struct AudioPlayback {
    mixer: Arc<AudioMixer>,
    subscriptions: BTreeMap<(String, String), Subscription>,
    cleanup: Arc<AudioCleanup>,
}

impl AudioPlayback {
    pub fn new(mixer: Arc<AudioMixer>) -> Self {
        Self::with_cleanup(mixer, Arc::new(AudioCleanup::default()))
    }

    pub(crate) fn with_cleanup(mixer: Arc<AudioMixer>, cleanup: Arc<AudioCleanup>) -> Self {
        Self {
            mixer,
            subscriptions: BTreeMap::new(),
            cleanup,
        }
    }
    pub fn attach(
        &mut self,
        peer: &str,
        track: RtcAudioTrack,
        lifetime: PeerLifetime,
        stream: bool,
    ) -> Result<(), MediaError> {
        let prepared = self
            .cleanup
            .prepare_task()
            .map_err(|_| MediaError::Backend)?;
        self.retain_alive();
        if !lifetime.is_alive() {
            return Err(MediaError::UnauthorizedPeer);
        }
        let key = (peer.to_owned(), track.id());
        if stream {
            self.subscriptions
                .retain(|(id, _), subscription| id != peer || !subscription.stream);
        }
        if self.subscriptions.len() >= 128 && !self.subscriptions.contains_key(&key) {
            return Err(MediaError::Backend);
        }
        self.subscriptions.remove(&key);
        let id = serde_json::to_string(&key).map_err(|_| MediaError::Backend)?;
        let lane = self
            .mixer
            .add(id.clone(), peer, lifetime.clone(), stream)
            .ok_or(MediaError::Backend)?;
        let mut audio = NativeAudioStream::with_options(
            track,
            48000,
            if stream { 2 } else { 1 },
            NativeAudioStreamOptions {
                queue_size_frames: Some(4),
            },
        );
        let stop = CancellationToken::new();
        let cancel = stop.clone();
        let output = lane.clone();
        let task = prepared.spawn(stop.clone(), async move {
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    _ = tick.tick() => { if !lifetime.is_alive() { break; } },
                    frame = audio.next() => {
                        let Some(frame) = frame else { break; };
                        if !lifetime.is_alive() || cancel.is_cancelled() { break; }
                        let channels = if stream { 2 } else { 1 };
                        if frame.sample_rate == 48000 && frame.num_channels == channels && frame.samples_per_channel as usize * channels as usize == frame.data.len() {
                            if stream { output.push_stereo(&frame.data); } else { output.push(&frame.data); }
                        }
                    }
                }
            }
            output.stop();
            audio.close();
        });
        self.subscriptions.insert(
            key,
            Subscription {
                id,
                lane,
                mixer: self.mixer.clone(),
                stop,
                task,
                stream,
            },
        );
        Ok(())
    }
    pub fn retain_alive(&mut self) {
        self.subscriptions.retain(|_, subscription| {
            !subscription.task.is_finished() && subscription.lane.is_active()
        });
    }
    pub fn clear(&mut self) {
        self.subscriptions.clear();
    }
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub async fn wait_for_cleanup(&self) -> Result<(), AudioCleanupError> {
        self.cleanup.wait().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_mix::AudioControls;
    use crate::audio_runtime::{SharedAudio, SharedAudioDevice};
    use crate::audio_settings::AudioSettings;
    use crate::media::MediaEngine;
    use crate::microphone::audio_source;
    use libwebrtc::data_channel::DataChannelInit;
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
    use libwebrtc::peer_connection_factory::PeerConnectionFactory;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    struct Device;

    impl SharedAudioDevice for Device {
        fn failed(&self) -> bool {
            false
        }

        fn cancel(&self) {}

        fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
            Ok(())
        }
    }

    fn peer() -> (MediaEngine, PeerLifetime) {
        let (mut engine, _) = MediaEngine::new_data_only("local".into());
        engine.authorize_peers(["remote".into()]);
        engine
            .create_data_channel(
                "remote",
                "playback-cleanup-fixture",
                DataChannelInit::default(),
            )
            .unwrap();
        let lifetime = engine.peer_lifetime("remote").unwrap();
        (engine, lifetime)
    }

    #[tokio::test]
    async fn stream_route_audio_replaces_only_the_same_peers_stream_lane_and_keeps_voice() {
        let controls = Arc::new(AudioControls::new(AudioSettings::default()));
        let mixer = Arc::new(AudioMixer::new(controls));
        let (engine, lifetime) = peer();
        let factory = PeerConnectionFactory::default();
        let voice = factory.create_audio_track("voice-fixture", audio_source());
        let first = factory.create_audio_track("stream-p2p-fixture", audio_source());
        let second = factory.create_audio_track("stream-sfu-fixture", audio_source());
        let mut playback = AudioPlayback::new(mixer);
        playback
            .attach("remote", voice, lifetime.clone(), false)
            .unwrap();
        playback
            .attach("remote", first, lifetime.clone(), true)
            .unwrap();
        playback
            .attach("remote", second, lifetime.clone(), true)
            .unwrap();
        assert_eq!(playback.subscription_count(), 2);
        assert!(playback
            .subscriptions
            .contains_key(&("remote".into(), "voice-fixture".into())));
        assert!(playback
            .subscriptions
            .contains_key(&("remote".into(), "stream-sfu-fixture".into())));
        assert!(!playback
            .subscriptions
            .contains_key(&("remote".into(), "stream-p2p-fixture".into())));
        assert!(lifetime.is_alive());
        playback.clear();
        playback.wait_for_cleanup().await.unwrap();
        drop(engine);
    }

    struct HeldDrop {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
        finished: Arc<AtomicBool>,
    }

    impl Drop for HeldDrop {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
            self.finished.store(true, Ordering::Release);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn subscription_cleanup_keeps_replacement_lane_voice_and_monitor_unchanged_while_task_drop_is_held(
    ) {
        let controls = Arc::new(AudioControls::new(AudioSettings::default()));
        let shared =
            SharedAudio::<Device>::new(controls.clone(), CancellationToken::new(), |_, _, _| {
                Box::pin(async { panic!("Playback fixture must not start physical audio") })
            });
        let cleanup = shared.cleanup_tracker();
        let mixer = Arc::new(AudioMixer::new(controls.clone()));
        let (mut engine, lifetime) = peer();
        let lane = mixer
            .add("lane".into(), "remote", lifetime.clone(), false)
            .unwrap();
        let stop = CancellationToken::new();
        let started = Arc::new(tokio::sync::Semaphore::new(0));
        let task_started = started.clone();
        let (drop_entered, drop_waiting) = std::sync::mpsc::channel();
        let (drop_release, drop_blocked) = std::sync::mpsc::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let probe = HeldDrop {
            entered: drop_entered,
            release: drop_blocked,
            finished: finished.clone(),
        };
        let task = cleanup
            .spawn_task(stop.clone(), async move {
                let _probe = probe;
                task_started.add_permits(1);
                std::future::pending::<()>().await;
            })
            .unwrap();
        let subscription = Subscription {
            id: "lane".into(),
            lane: lane.clone(),
            mixer: mixer.clone(),
            stop: stop.clone(),
            task,
            stream: false,
        };
        started.acquire().await.unwrap().forget();
        let fresh = mixer
            .add("lane".into(), "remote", lifetime.clone(), false)
            .unwrap();
        let monitor = shared.monitor(true);
        let generation = shared.demand.reserve().unwrap();
        assert!(shared.demand.activate(generation));
        controls.speaking.store(true, Ordering::Release);
        controls
            .mic_db
            .store((-17.0_f32).to_bits(), Ordering::Release);
        let epoch = controls.capture_epoch.load(Ordering::Acquire);
        let snapshot = shared.mic_test.snapshot(Instant::now());
        fresh.push(&[8192; 480]);
        drop(subscription);
        tokio::task::spawn_blocking(move || drop_waiting.recv_timeout(Duration::from_secs(1)))
            .await
            .unwrap()
            .unwrap();
        assert!(stop.is_cancelled());
        assert!(!lane.is_active());
        assert!(fresh.is_active());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), shared.wait_for_cleanup())
                .await
                .is_err()
        );
        assert!(!finished.load(Ordering::Acquire));
        let mut pcm = [0.0; 960];
        mixer.render_pcm(&mut pcm);
        assert!(pcm.iter().any(|sample| *sample != 0.0));
        drop_release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), shared.wait_for_cleanup())
            .await
            .unwrap()
            .unwrap();
        assert!(finished.load(Ordering::Acquire));
        assert!(fresh.is_active());
        assert!(lifetime.is_alive());
        assert_eq!(shared.demand.active(), generation);
        assert!(shared.demand.is_reserved(generation));
        assert!(monitor.is_current());
        assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
        assert!(controls.speaking.load(Ordering::Acquire));
        assert_eq!(
            f32::from_bits(controls.mic_db.load(Ordering::Acquire)),
            -17.0
        );
        assert_eq!(controls.capture_epoch.load(Ordering::Acquire), epoch);
        engine.close();
        println!("playback cleanup held_task_drop=true original_lane_only=true fresh_lane_voice_monitor_unchanged=true shared_drain_waited=true peer_ticket_unchanged=true physical_audio=false");
    }

    #[tokio::test]
    async fn sealed_shared_playback_rejects_attachment_before_replacing_existing_native_stream_or_lane(
    ) {
        let controls = Arc::new(AudioControls::new(AudioSettings::default()));
        let shared =
            SharedAudio::<Device>::new(controls.clone(), CancellationToken::new(), |_, _, _| {
                Box::pin(async { panic!("Sealed playback fixture must not start physical audio") })
            });
        let mixer = Arc::new(AudioMixer::new(controls.clone()));
        let factory = PeerConnectionFactory::default();
        let source = audio_source();
        let track = factory.create_audio_track("sealed-playback", source.clone());
        let (mut engine, lifetime) = peer();
        let mut playback = AudioPlayback::with_cleanup(mixer, shared.cleanup_tracker());
        playback
            .attach("remote", track.clone(), lifetime.clone(), false)
            .unwrap();
        let lane = playback.subscriptions.values().next().unwrap().lane.clone();
        let stop = playback.subscriptions.values().next().unwrap().stop.clone();
        let monitor = shared.monitor(true);
        let generation = shared.demand.reserve().unwrap();
        assert!(shared.demand.activate(generation));
        controls.speaking.store(true, Ordering::Release);
        controls
            .mic_db
            .store((-17.0_f32).to_bits(), Ordering::Release);
        let epoch = controls.capture_epoch.load(Ordering::Acquire);
        let snapshot = shared.mic_test.snapshot(Instant::now());
        shared.seal_cleanup().unwrap();
        shared.seal_cleanup().unwrap();
        assert_eq!(
            playback.attach("remote", track.clone(), lifetime.clone(), false),
            Err(MediaError::Backend)
        );
        assert_eq!(playback.subscription_count(), 1);
        assert!(Arc::ptr_eq(
            &playback.subscriptions.values().next().unwrap().lane,
            &lane
        ));
        assert!(lane.is_active());
        assert!(!stop.is_cancelled());
        assert!(lifetime.is_alive());
        assert_eq!(shared.demand.active(), generation);
        assert!(shared.demand.is_reserved(generation));
        assert!(monitor.is_current());
        assert_eq!(shared.mic_test.snapshot(Instant::now()), snapshot);
        assert!(controls.speaking.load(Ordering::Acquire));
        assert_eq!(
            f32::from_bits(controls.mic_db.load(Ordering::Acquire)),
            -17.0
        );
        assert_eq!(controls.capture_epoch.load(Ordering::Acquire), epoch);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), shared.wait_for_cleanup())
                .await
                .is_err()
        );
        playback.clear();
        tokio::time::timeout(Duration::from_secs(2), shared.wait_for_cleanup())
            .await
            .unwrap()
            .unwrap();
        assert!(stop.is_cancelled());
        assert!(!lane.is_active());
        assert_eq!(
            playback.attach("remote", track.clone(), lifetime.clone(), false),
            Err(MediaError::Backend)
        );
        assert_eq!(playback.subscription_count(), 0);
        assert!(shared.wait_for_cleanup().await.is_ok());
        engine.close();
        println!("audio admission playback=true sealed_before_attach=true old_stream_lane_untouched=true voice_monitor_status_unchanged=true admitted_clear_join_waited=true late_attach_rejected=true physical_audio=false");
    }

    #[tokio::test]
    async fn actual_native_stream_tasks_are_joined_after_replacement_clear_peer_retention_and_playback_drop(
    ) {
        for mode in ["replace", "clear", "peer-retain", "drop"] {
            let controls = Arc::new(AudioControls::new(AudioSettings::default()));
            let shared = SharedAudio::<Device>::new(
                controls.clone(),
                CancellationToken::new(),
                |_, _, _| {
                    Box::pin(async {
                        panic!("Native stream fixture must not start physical audio")
                    })
                },
            );
            let mixer = Arc::new(AudioMixer::new(controls));
            let factory = PeerConnectionFactory::default();
            let source = audio_source();
            let track = factory.create_audio_track("playback-cleanup", source.clone());
            let (mut engine, lifetime) = peer();
            let mut playback = AudioPlayback::with_cleanup(mixer.clone(), shared.cleanup_tracker());
            playback
                .attach("remote", track.clone(), lifetime.clone(), false)
                .unwrap();
            assert_eq!(playback.subscription_count(), 1);
            if mode == "replace" {
                playback
                    .attach("remote", track.clone(), lifetime.clone(), false)
                    .unwrap();
                assert_eq!(playback.subscription_count(), 1);
                assert!(tokio::time::timeout(
                    Duration::from_millis(30),
                    playback.wait_for_cleanup()
                )
                .await
                .is_err());
                assert!(lifetime.is_alive());
                playback.clear();
            } else if mode == "peer-retain" {
                engine.disconnect_peer("remote");
                assert!(!lifetime.is_alive());
                playback.retain_alive();
            } else if mode == "clear" {
                playback.clear();
            }
            if mode != "drop" {
                assert_eq!(playback.subscription_count(), 0);
                tokio::time::timeout(Duration::from_secs(2), playback.wait_for_cleanup())
                    .await
                    .unwrap()
                    .unwrap();
            }
            drop(playback);
            tokio::time::timeout(Duration::from_secs(2), shared.wait_for_cleanup())
                .await
                .unwrap()
                .unwrap();
            assert!(shared.wait_for_cleanup().await.is_ok());
            let mut pcm = [1.0; 960];
            mixer.render_pcm(&mut pcm);
            assert_eq!(pcm, [0.0; 960]);
            assert_eq!(track.id(), "playback-cleanup");
            drop(track);
            drop(source);
            engine.close();
            println!("playback cleanup native_stream=true detach={mode} observed_task_join=true external_track_source_retained=true shared_drain_waited=true last_arc_bounds=false physical_audio=false");
        }
    }
}
