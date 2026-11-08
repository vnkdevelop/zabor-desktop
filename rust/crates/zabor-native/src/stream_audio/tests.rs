use super::*;

#[test]
fn stereo_queue_preserves_channels_sanitizes_bounds_and_discards_partial_discontinuous_audio() {
    let mut queue = PcmQueue(VecDeque::new());
    queue
        .push(Packet {
            samples: (0..480).flat_map(|_| [-0.25, 0.5]).collect(),
            discontinuity: false,
        })
        .unwrap();
    let frame = queue.frame().unwrap();
    assert!(frame
        .as_chunks::<2>()
        .0
        .iter()
        .all(|pair| *pair == [-8192, 16384]));
    queue
        .push(Packet {
            samples: vec![0.75; 480],
            discontinuity: false,
        })
        .unwrap();
    queue
        .push(Packet {
            samples: (0..480).flat_map(|_| [f32::NAN, 2.0]).collect(),
            discontinuity: true,
        })
        .unwrap();
    assert!(queue
        .frame()
        .unwrap()
        .as_chunks::<2>()
        .0
        .iter()
        .all(|pair| *pair == [0, 32767]));
    queue
        .push(Packet {
            samples: vec![0.25; 19200],
            discontinuity: false,
        })
        .unwrap();
    assert_eq!(queue.0.len(), QUEUE_SAMPLES);
    assert!(queue
        .push(Packet {
            samples: vec![0.0; 3],
            discontinuity: false
        })
        .is_err());
    assert!(queue
        .push(Packet {
            samples: vec![0.0; 19202],
            discontinuity: false
        })
        .is_err());
}

#[cfg(feature = "stream-audio-test-source")]
mod integration {
    use super::*;
    use futures_util::StreamExt;
    use libwebrtc::audio_stream::native::NativeAudioStream;
    use libwebrtc::peer_connection_factory::{
        native::PeerConnectionFactoryExt, PeerConnectionFactory,
    };
    use std::time::{Duration, Instant};
    use tokio::time::timeout;

    #[tokio::test]
    async fn isolated_source_delivers_stereo_without_apm_and_registered_drain_waits_for_owner() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let capture = StreamAudioCapture::synthetic(cleanup.clone(), 2, 150)
                .await
                .unwrap();
            assert!(capture.is_live());
            assert_eq!(capture.source.sample_rate(), 48000);
            assert_eq!(capture.source.num_channels(), 2);
            let options = capture.source.audio_options();
            assert!(
                !options.echo_cancellation
                    && !options.noise_suppression
                    && !options.auto_gain_control
            );
            let factory = PeerConnectionFactory::default();
            let track =
                factory.create_audio_track("isolated-audio-fixture", capture.source.clone());
            let mut audio = NativeAudioStream::new(track, 48000, 2);
            let frame = audio.next().await.unwrap();
            assert_eq!(frame.num_channels, 2);
            assert_eq!(frame.data.len(), frame.samples_per_channel as usize * 2);
            assert!(frame
                .data
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| *pair == [-8192, 16384]));
            audio.close();
            drop(audio);
            let started = Instant::now();
            drop(capture);
            assert!(started.elapsed() < Duration::from_millis(100));
            cleanup.seal().unwrap();
            assert!(timeout(Duration::from_millis(80), cleanup.wait())
                .await
                .is_err());
            cleanup.wait().await.unwrap();
            assert!(started.elapsed() >= Duration::from_millis(150));
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn pre_cancelled_and_sealed_registration_do_not_acquire_source_or_native_audio() {
        for sealed in [true, false] {
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            if sealed {
                cleanup.seal().unwrap();
            } else {
                stop.cancel();
            }
            assert!(
                StreamAudioCapture::start_with(cleanup.clone(), stop, |_, _, _| panic!(
                    "No acquisition allowed"
                ))
                .await
                .is_err()
            );
            timeout(Duration::from_secs(5), cleanup.wait())
                .await
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn cancelled_start_wait_keeps_original_actor_registration_across_late_acquisition() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let stop = CancellationToken::new();
            let (entered, began) = oneshot::channel();
            let (release, blocked) = std::sync::mpsc::channel();
            let task_cleanup = cleanup.clone();
            let task_stop = stop.clone();
            let task = tokio::spawn(async move {
                StreamAudioCapture::start_with(
                    task_cleanup,
                    task_stop,
                    move |stop, source, ready| {
                        entered.send(()).unwrap();
                        blocked.recv().unwrap();
                        forward(stop, source, ready, |cancelled, ready, deliver| {
                            zabor_stream_audio::run_synthetic(
                                Source::Screen,
                                0,
                                0,
                                cancelled,
                                ready,
                                deliver,
                            )
                        })
                    },
                )
                .await
            });
            began.await.unwrap();
            stop.cancel();
            assert!(task.await.unwrap().is_err());
            cleanup.seal().unwrap();
            assert!(timeout(Duration::from_millis(50), cleanup.wait())
                .await
                .is_err());
            release.send(()).unwrap();
            cleanup.wait().await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn acquisition_failure_and_cleanup_failure_remain_distinct_in_application_drain() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            assert!(StreamAudioCapture::synthetic(cleanup.clone(), 3, 0)
                .await
                .is_err());
            cleanup.wait().await.unwrap();
            let cleanup = Arc::new(AudioCleanup::default());
            let capture = StreamAudioCapture::synthetic(cleanup.clone(), 5, 0)
                .await
                .unwrap();
            drop(capture);
            assert!(cleanup.wait().await.is_err());
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn actor_unwind_waits_for_backend_retirement_and_poisoned_original_join() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let (entered, began) = oneshot::channel();
            cleanup
                .prepare_task()
                .unwrap()
                .spawn_blocking_result(move || {
                    zabor_stream_audio::run_synthetic(
                        Source::Screen,
                        2,
                        150,
                        || false,
                        |_| {
                            entered.send(()).unwrap();
                            true
                        },
                        |_| panic!("Synthetic registered isolated audio unwind"),
                    )
                    .unwrap();
                    Ok(())
                });
            began.await.unwrap();
            let started = Instant::now();
            assert!(timeout(Duration::from_millis(80), cleanup.wait())
                .await
                .is_err());
            assert!(cleanup.wait().await.is_err());
            assert!(started.elapsed() >= Duration::from_millis(150));
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn leave_and_logout_revoke_original_audio_start_without_aborting_its_owner() {
        use crate::critical_intents::{CriticalAction, CriticalIntents, IntentScope};
        timeout(Duration::from_secs(5), async {
            for action in [CriticalAction::Leave, CriticalAction::Logout] {
                let intents = CriticalIntents::default();
                let ticket = intents.ticket();
                let session = intents.session_token(ticket);
                let cleanup = Arc::new(AudioCleanup::default());
                let (entered, began) = oneshot::channel();
                let (release, blocked) = std::sync::mpsc::channel();
                let (result, ()) = tokio::join!(
                    intents.run(
                        ticket,
                        IntentScope::Voice,
                        StreamAudioCapture::start_with(
                            cleanup.clone(),
                            CancellationToken::new(),
                            move |stop, source, ready| {
                                entered.send(()).unwrap();
                                blocked.recv().unwrap();
                                forward(stop, source, ready, |cancelled, ready, deliver| {
                                    zabor_stream_audio::run_synthetic(
                                        Source::Screen,
                                        0,
                                        0,
                                        cancelled,
                                        ready,
                                        deliver,
                                    )
                                })
                            }
                        )
                    ),
                    async {
                        began.await.unwrap();
                        intents.request(action);
                    }
                );
                assert!(result.is_none());
                assert_eq!(session.is_cancelled(), action == CriticalAction::Logout);
                cleanup.seal().unwrap();
                assert!(timeout(Duration::from_millis(50), cleanup.wait())
                    .await
                    .is_err());
                release.send(()).unwrap();
                cleanup.wait().await.unwrap();
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn aborting_start_wait_does_not_complete_or_abort_the_original_blocking_actor() {
        timeout(Duration::from_secs(5), async {
            let cleanup = Arc::new(AudioCleanup::default());
            let task_cleanup = cleanup.clone();
            let (entered, began) = oneshot::channel();
            let (release, blocked) = std::sync::mpsc::channel();
            let task = tokio::spawn(async move {
                StreamAudioCapture::start_with(
                    task_cleanup,
                    CancellationToken::new(),
                    move |stop, source, ready| {
                        entered.send(()).unwrap();
                        blocked.recv().unwrap();
                        forward(stop, source, ready, |cancelled, ready, deliver| {
                            zabor_stream_audio::run_synthetic(
                                Source::Screen,
                                0,
                                0,
                                cancelled,
                                ready,
                                deliver,
                            )
                        })
                    },
                )
                .await
            });
            began.await.unwrap();
            task.abort();
            assert!(task.await.is_err());
            cleanup.seal().unwrap();
            assert!(timeout(Duration::from_millis(50), cleanup.wait())
                .await
                .is_err());
            release.send(()).unwrap();
            cleanup.wait().await.unwrap();
        })
        .await
        .unwrap();
    }

    struct Device;

    impl crate::audio_runtime::SharedAudioDevice for Device {
        fn failed(&self) -> bool {
            false
        }
        fn cancel(&self) {}
        fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn isolated_capture_start_stop_preserves_existing_voice_monitor_and_mic_test_owner() {
        use crate::audio_mix::AudioControls;
        use crate::audio_runtime::SharedAudio;
        use crate::audio_settings::AudioSettings;
        timeout(Duration::from_secs(5), async {
            let controls = Arc::new(AudioControls::new(AudioSettings::default()));
            let runtime =
                SharedAudio::new(controls.clone(), CancellationToken::new(), |_, _, _| {
                    Box::pin(async { Ok(Device) })
                });
            let voice = runtime.voice().await.unwrap();
            assert!(voice.activate());
            let monitor = runtime.monitor(true);
            let device = monitor.ready().await.unwrap();
            let generation = runtime.demand.active();
            let epoch = controls.capture_epoch.load(Ordering::Acquire);
            let capture = StreamAudioCapture::synthetic(runtime.cleanup_tracker(), 0, 0)
                .await
                .unwrap();
            drop(capture);
            runtime.wait_for_cleanup().await.unwrap();
            assert_eq!(runtime.demand.active(), generation);
            assert_eq!(controls.capture_epoch.load(Ordering::Acquire), epoch);
            assert!(!voice.failed() && monitor.is_current() && !monitor.failed());
            assert!(Arc::ptr_eq(voice.device().unwrap(), &device));
            drop(voice);
            assert!(monitor.is_current());
            drop(monitor);
            drop(device);
            runtime.wait_for_cleanup().await.unwrap();
        })
        .await
        .unwrap();
    }
}
