use std::time::{Duration, Instant};
use zabor_native::mic_test::{
    CaptureToken, MicTest, MicTestFailure, MicTestIssue, MicTestStage, CAPTURE_TIMEOUT,
    CLIP_SAMPLES, MONITOR_SETTLE, PIPELINE_FLUSH, PREPARATION_TIMEOUT, SAMPLE_RATE,
};

fn prepared(test: &MicTest, now: Instant) -> CaptureToken {
    test.configure(true, true);
    let token = test.begin(now).unwrap();
    assert!(test.prepared(token, now));
    test.snapshot(now + MONITOR_SETTLE);
    token
}

fn recorded(pcm: [f32; 480]) -> (MicTest, Instant) {
    let test = MicTest::default();
    let now = Instant::now();
    let token = prepared(&test, now);
    for index in 0..CLIP_SAMPLES / 480 {
        assert!(test.capture(
            token,
            &pcm,
            -20.0,
            now + MONITOR_SETTLE + PIPELINE_FLUSH + Duration::from_millis(index as u64 * 10),
        ));
    }
    (test, now + Duration::from_secs(6))
}

#[test]
fn recording_uses_exact_reference_settle_flush_frame_count_and_one_memory_buffer() {
    let test = MicTest::default();
    let now = Instant::now();
    test.configure(true, true);
    let token = test.begin(now).unwrap();
    assert_eq!(test.begin(now), Some(token));
    let state = test.snapshot(now);
    assert_eq!(state.stage, MicTestStage::Arming);
    assert_eq!(state.stored_samples, 240000);
    assert_eq!(state.stored_samples * size_of::<f32>(), 960000);
    assert!(test.prepared(token, now));
    assert!(test.capture(token, &[0.8; 480], -3.0, now + Duration::from_millis(119)));
    assert_eq!(
        test.snapshot(now + Duration::from_millis(119)).stage,
        MicTestStage::Arming
    );
    assert_eq!(
        test.snapshot(now + MONITOR_SETTLE).stage,
        MicTestStage::Recording
    );
    assert!(test.capture(token, &[0.8; 480], -3.0, now + Duration::from_millis(399)));
    assert_eq!(
        test.snapshot(now + Duration::from_millis(399))
            .captured_samples,
        0
    );
    for index in 0..500 {
        assert!(test.capture(
            token,
            &[0.000005; 480],
            -80.0,
            now + Duration::from_millis(400 + index * 10),
        ));
    }
    let state = test.snapshot(now + Duration::from_secs(6));
    assert_eq!(state.stage, MicTestStage::Ready);
    assert_eq!(state.captured_samples, CLIP_SAMPLES);
    assert_eq!(state.stored_samples, CLIP_SAMPLES);
    assert_eq!(state.duration_seconds, 5.0);
    assert!(state.playing);
    let mut stereo = [0.0; 960];
    test.render(&mut stereo, 0, 1.0).unwrap();
    assert!(stereo.iter().all(|sample| *sample == 0.000005));
}

#[test]
fn delayed_recording_callback_still_requires_a_full_flush_without_extending_capture_timeout() {
    let test = MicTest::default();
    let now = Instant::now();
    test.configure(true, true);
    let token = test.begin(now).unwrap();
    assert!(test.prepared(token, now));
    let late = now + Duration::from_secs(1);
    assert!(test.capture(token, &[0.1; 480], -20.0, late));
    assert_eq!(test.snapshot(late).stage, MicTestStage::Recording);
    assert!(test.capture(token, &[0.1; 480], -20.0, late + Duration::from_millis(279)));
    assert_eq!(
        test.snapshot(late + Duration::from_millis(279))
            .captured_samples,
        0
    );
    assert!(test.capture(token, &[0.1; 480], -20.0, late + PIPELINE_FLUSH));
    assert_eq!(test.snapshot(late + PIPELINE_FLUSH).captured_samples, 480);
    let failed = test.snapshot(now + CAPTURE_TIMEOUT);
    assert_eq!(failed.stage, MicTestStage::Idle);
    assert_eq!(failed.failure, Some(MicTestFailure::CaptureTimeout));
    assert_eq!(failed.stored_samples, 0);
    assert!(test.capture_token().is_some());
    assert!(!test.capture(token, &[0.1; 480], -20.0, now + CAPTURE_TIMEOUT));
}

#[test]
fn preparation_and_capture_deadlines_are_separate_and_duplicate_ready_does_not_extend_them() {
    let test = MicTest::default();
    let now = Instant::now();
    test.configure(true, true);
    let expired = test.begin(now).unwrap();
    let failed = test.snapshot(now + PREPARATION_TIMEOUT);
    assert_eq!(failed.failure, Some(MicTestFailure::PreparationTimeout));
    assert!(!test.prepared(expired, now + PREPARATION_TIMEOUT));
    let token = test.begin(now + PREPARATION_TIMEOUT).unwrap();
    let attached = now + PREPARATION_TIMEOUT + Duration::from_secs(29);
    assert!(test.prepared(token, attached));
    assert!(test.prepared(token, attached + Duration::from_secs(8)));
    assert_eq!(
        test.snapshot(attached + CAPTURE_TIMEOUT).failure,
        Some(MicTestFailure::CaptureTimeout)
    );
}

#[test]
fn silence_uses_rounded_peak_not_rms_and_automatic_play_requires_visibility() {
    for (db, expected, issue) in [
        (-70.1, -70, Some(MicTestIssue::Silent)),
        (-69.51, -70, Some(MicTestIssue::Silent)),
        (-69.49, -69, None),
    ] {
        let amplitude = 10.0_f32.powf(db / 20.0);
        let (test, now) = recorded([amplitude; 480]);
        let state = test.snapshot(now);
        assert_eq!(state.peak_db, Some(expected));
        assert_eq!(state.issue, issue);
    }
    let (silent, now) = recorded([0.0; 480]);
    assert_eq!(silent.snapshot(now).peak_db, Some(-120));
    assert_eq!(silent.snapshot(now).issue, Some(MicTestIssue::Silent));
    let test = MicTest::default();
    let now = Instant::now();
    let token = prepared(&test, now);
    test.configure(true, false);
    let mut impulse = [0.0; 480];
    impulse[0] = 0.01;
    for index in 0..500 {
        assert!(test.capture(
            token,
            if index == 0 { &impulse } else { &[0.0; 480] },
            -100.0,
            now + Duration::from_millis(400 + index * 10),
        ));
    }
    let state = test.snapshot(now + Duration::from_secs(6));
    assert_eq!(state.peak_db, Some(-40));
    assert_eq!(state.issue, None);
    assert_eq!(state.stage, MicTestStage::Ready);
    assert!(!state.playing);
    assert!(!test.has_demand());
    test.configure(true, true);
    assert!(!test.snapshot(now + Duration::from_secs(6)).playing);
    assert!(test.play(None));
}

#[test]
fn tab_hide_pauses_retains_the_clip_and_disable_or_dispose_revokes_every_old_callback() {
    let (test, now) = recorded([0.1; 480]);
    let mut stereo = [0.0; 960];
    let ticket = test.render(&mut stereo, 2, 1.0).unwrap();
    test.configure(true, false);
    assert!(!test.validate_playback(ticket, 2));
    assert!(!test.played(ticket, 2));
    assert!(!test.play(None));
    assert_eq!(test.snapshot(now).stored_samples, CLIP_SAMPLES);
    test.configure(true, true);
    assert!(test.play(None));
    let ticket = test.render(&mut stereo, 2, 1.0).unwrap();
    test.configure(false, true);
    test.configure(true, true);
    assert!(!test.played(ticket, 2));
    assert_eq!(test.snapshot(now).stored_samples, 0);
    let old = test.begin(now).unwrap();
    assert!(test.prepared(old, now));
    test.dispose();
    test.configure(true, true);
    let current = test.begin(now).unwrap();
    assert_ne!(current, old);
    assert!(!test.prepared(old, now));
    assert!(!test.capture(old, &[0.2; 480], -10.0, now + Duration::from_millis(400)));
    test.fail(old, MicTestFailure::Processor);
    assert_eq!(test.snapshot(now).stage, MicTestStage::Arming);
    assert_eq!(test.snapshot(now).failure, None);
}

#[test]
fn playback_cursor_advances_only_for_the_latest_successfully_written_ticket() {
    let (test, now) = recorded([0.2; 480]);
    let mut stereo = [0.0; 960];
    let old = test.render(&mut stereo, 1, 1.0).unwrap();
    let current = test.render(&mut stereo, 1, 1.0).unwrap();
    assert!(!test.validate_playback(old, 1));
    assert!(!test.played(old, 1));
    assert_eq!(test.snapshot(now).position_seconds, 0.0);
    assert!(test.validate_playback(current, 1));
    assert!(test.played(current, 1));
    assert!(!test.played(current, 1));
    assert_eq!(test.snapshot(now).position_seconds, 0.01);
    let old_route = test.render(&mut stereo, 1, 1.0).unwrap();
    assert!(!test.played(old_route, 2));
    let current_route = test.render(&mut stereo, 2, 1.0).unwrap();
    assert!(!test.validate_playback(old_route, 1));
    assert!(test.played(current_route, 2));
    assert_eq!(test.snapshot(now).position_seconds, 0.02);
    let paused = test.render(&mut stereo, 2, 1.0).unwrap();
    test.pause();
    assert!(!test.played(paused, 2));
    assert_eq!(test.snapshot(now).position_seconds, 0.02);
}

#[test]
fn playback_preserves_float_samples_uses_only_global_gain_and_has_exact_seek_end_replay_rules() {
    let (test, now) = recorded([0.4; 480]);
    let mut stereo = [0.0; 960];
    let first = test.render(&mut stereo, 0, 2.0).unwrap();
    assert!(stereo.iter().all(|sample| *sample == 0.8));
    assert!(test.played(first, 0));
    test.pause();
    assert!(test.seek(0.5));
    assert_eq!(test.snapshot(now).position_seconds, 0.5);
    assert!(!test.snapshot(now).playing);
    assert!(test.play(None));
    let stale = test.render(&mut stereo, 0, 1.0).unwrap();
    assert!(test.seek(5.0));
    assert!(!test.played(stale, 0));
    assert_eq!(test.snapshot(now).position_seconds, 4.95);
    for _ in 0..5 {
        let ticket = test.render(&mut stereo, 0, 1.0).unwrap();
        assert!(test.played(ticket, 0));
    }
    assert_eq!(test.snapshot(now).position_seconds, 5.0);
    assert!(!test.snapshot(now).playing);
    assert!(test.render(&mut stereo, 0, 1.0).is_none());
    assert_eq!(stereo, [0.0; 960]);
    assert!(test.play(None));
    assert_eq!(test.snapshot(now).position_seconds, 0.0);
    assert!(test.play(Some(4.98)));
    assert_eq!(test.snapshot(now).position_seconds, 0.0);
    assert!(test.seek(-100.0));
    assert_eq!(test.snapshot(now).position_seconds, 0.0);
}

#[test]
fn oversized_odd_or_nonfinite_render_requests_cannot_move_the_cursor_or_emit_invalid_audio() {
    let (test, now) = recorded([-0.8; 480]);
    for (length, gain) in [(3, 1.0), (SAMPLE_RATE * 2 + 2, 1.0), (960, f32::NAN)] {
        let mut stereo = vec![1.0; length];
        assert!(test.render(&mut stereo, 0, gain).is_none());
        assert!(stereo.iter().all(|sample| *sample == 0.0));
        assert_eq!(test.snapshot(now).position_seconds, 0.0);
    }
    assert!(!test.seek(f64::NAN));
    assert!(!test.play(Some(f64::INFINITY)));
    let mut stereo = [0.0; 960];
    assert!(test.render(&mut stereo, 0, 2.0).is_some());
    assert_eq!(stereo, [-1.0; 960]);
}

#[test]
fn invalid_frames_and_processor_failures_clear_the_clip_and_old_failures_do_not_cancel_retakes() {
    let test = MicTest::default();
    let now = Instant::now();
    let old = prepared(&test, now);
    assert!(!test.capture(
        old,
        &[f32::NAN; 480],
        -20.0,
        now + Duration::from_millis(400)
    ));
    assert_eq!(
        test.snapshot(now).failure,
        Some(MicTestFailure::InvalidFrame)
    );
    assert_eq!(test.snapshot(now).stored_samples, 0);
    let token = test.begin(now).unwrap();
    assert_ne!(token, old);
    test.fail(old, MicTestFailure::Processor);
    assert_eq!(test.snapshot(now).failure, None);
    test.fail(token, MicTestFailure::Processor);
    assert_eq!(test.snapshot(now).failure, Some(MicTestFailure::Processor));
    assert!(test.render(&mut [1.0; 960], 0, 1.0).is_none());
}

#[cfg(feature = "native-media")]
mod media {
    use super::*;
    use futures_util::StreamExt;
    use libwebrtc::audio_stream::native::NativeAudioStream;
    use libwebrtc::peer_connection_factory::{
        native::PeerConnectionFactoryExt, PeerConnectionFactory,
    };
    use std::sync::{atomic::Ordering, Arc};
    use tokio_util::sync::CancellationToken;
    use zabor_native::audio_mix::{AudioControls, AudioMixer};
    use zabor_native::audio_settings::AudioSettings;
    use zabor_native::microphone::{audio_source, MicProcessor, MicSubmission};

    fn microphone(settings: AudioSettings) -> (Arc<AudioControls>, MicProcessor) {
        let controls = Arc::new(AudioControls::new(settings));
        let mixer = Arc::new(AudioMixer::new(controls.clone()));
        let processor = MicProcessor::new(controls.clone(), mixer).unwrap();
        (controls, processor)
    }

    #[test]
    fn local_capture_survives_user_and_server_mute_without_changing_any_network_controls() {
        let (controls, mut processor) = microphone(AudioSettings {
            noise_suppression: false,
            ..AudioSettings::default()
        });
        let test = MicTest::default();
        let now = Instant::now();
        let token = prepared(&test, now);
        for (index, (user, server)) in [
            ((true, false), (false, false)),
            ((false, true), (false, false)),
            ((false, false), (true, false)),
            ((false, false), (false, true)),
            ((true, true), (true, true)),
        ]
        .into_iter()
        .enumerate()
        {
            controls.set_user_state(user.0, user.1);
            controls.set_server_state(server.0, server.1);
            let frame = processor.process_monitored(&[0.2; 480], 1, false);
            assert_eq!(frame.pcm, [0; 480]);
            assert!(frame
                .local
                .unwrap()
                .iter()
                .all(|sample| *sample == 0.2 * 1.7));
            assert!(frame.local_metrics.db > -20.0);
            assert!(test.capture(
                token,
                &frame.local.unwrap(),
                frame.local_metrics.db,
                now + Duration::from_millis(400 + index as u64 * 10)
            ));
            assert_eq!(controls.user_state(), user);
            assert_eq!(controls.server_state(), server);
            assert!(controls.effectively_muted());
            assert!(!controls.speaking.load(Ordering::Acquire));
            assert_eq!(
                f32::from_bits(controls.mic_db.load(Ordering::Acquire)),
                -100.0
            );
        }
        assert_eq!(
            test.snapshot(now + Duration::from_secs(1)).captured_samples,
            5 * 480
        );
        assert_eq!(processor.process(&[0.2; 480], 1, false), [0; 480]);
    }

    #[test]
    fn monitored_float_output_matches_the_existing_gain_peak_and_ultra_branches_before_quantization(
    ) {
        for settings in [
            AudioSettings {
                noise_suppression: false,
                input_volume: 50.0,
                ..AudioSettings::default()
            },
            AudioSettings {
                noise_suppression: false,
                input_volume: 200.0,
                ..AudioSettings::default()
            },
            AudioSettings {
                ultra_low_latency: true,
                input_volume: 0.0,
                ..AudioSettings::default()
            },
        ] {
            let (_, mut regular) = microphone(settings.clone());
            let (_, mut monitored) = microphone(settings);
            let input = std::array::from_fn::<_, 480, _>(|index| (index as f32 * 0.1).sin() * 0.9);
            let expected = regular.process(&input, 1, false);
            let actual = monitored.process_monitored(&input, 1, false);
            assert_eq!(actual.pcm, expected);
            for (sample, expected) in actual.local.unwrap().iter().zip(expected) {
                let converted = (f64::from(*sample) * if *sample < 0.0 { 32768.0 } else { 32767.0 })
                    .round() as i16;
                assert_eq!(converted, expected);
            }
        }
    }

    #[test]
    fn invalid_or_old_device_frames_never_enter_the_local_recording_and_recovery_preserves_the_buffer(
    ) {
        let (controls, mut processor) = microphone(AudioSettings {
            noise_suppression: false,
            ..AudioSettings::default()
        });
        let test = MicTest::default();
        let now = Instant::now();
        let token = prepared(&test, now);
        let good = processor.process_monitored(&[0.1; 480], 1, false);
        assert!(test.capture(
            token,
            &good.local.unwrap(),
            good.local_metrics.db,
            now + Duration::from_millis(400)
        ));
        controls.apply_settings(AudioSettings {
            noise_suppression: false,
            selected_input: "preferred-new-input".into(),
            ..AudioSettings::default()
        });
        assert!(processor
            .process_monitored(&[0.1; 480], 1, false)
            .local
            .is_none());
        assert_eq!(
            test.snapshot(now + Duration::from_millis(500))
                .captured_samples,
            480
        );
        controls.input_route_epoch.store(
            controls.input_device_epoch.load(Ordering::Acquire),
            Ordering::Release,
        );
        controls.capture_restarting();
        let current = processor.process_monitored(&[0.1; 480], 1, false);
        assert!(test.capture(
            token,
            &current.local.unwrap(),
            current.local_metrics.db,
            now + Duration::from_millis(510)
        ));
        let invalid = processor.process_monitored(&[f32::NAN; 480], 1, false);
        assert_eq!(invalid.pcm, [0; 480]);
        assert!(invalid.local.is_none());
        assert!(processor
            .process_monitored(&[0.1; 480], 1, true)
            .local
            .is_some());
        assert_eq!(
            test.snapshot(now + Duration::from_secs(1)).captured_samples,
            960
        );
    }

    #[tokio::test]
    async fn final_submission_still_rejects_monitored_pcm_after_short_mute_toggle_and_cancellation()
    {
        let (controls, mut processor) = microphone(AudioSettings {
            noise_suppression: false,
            ..AudioSettings::default()
        });
        let source = audio_source();
        let track = PeerConnectionFactory::default()
            .create_audio_track("monitor-local-only", source.clone());
        let mut output = NativeAudioStream::new(track, 48000, 1);
        let stop = CancellationToken::new();
        let submit = MicSubmission::new(source, controls.clone(), stop.clone());
        let mut frame = processor.process_monitored(&[0.1; 480], 1, false);
        assert!(frame.local.is_some());
        controls.set_user_state(true, false);
        controls.set_user_state(false, false);
        submit.submit(&mut frame.pcm, frame.epoch).unwrap();
        assert_eq!(frame.pcm, [0; 480]);
        let received = tokio::time::timeout(Duration::from_secs(1), output.next())
            .await
            .unwrap()
            .unwrap();
        assert!(received.data.iter().all(|sample| *sample == 0));
        controls.set_server_state(false, true);
        let mut frame = processor.process_monitored(&[0.1; 480], 1, false);
        assert!(frame.local.is_some());
        submit.submit(&mut frame.pcm, frame.epoch).unwrap();
        assert_eq!(frame.pcm, [0; 480]);
        let received = tokio::time::timeout(Duration::from_secs(1), output.next())
            .await
            .unwrap()
            .unwrap();
        assert!(received.data.iter().all(|sample| *sample == 0));
        stop.cancel();
        let mut frame = processor.process_monitored(&[0.1; 480], 1, false);
        submit.submit(&mut frame.pcm, frame.epoch).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), output.next())
                .await
                .is_err()
        );
        output.close();
    }

    #[cfg(feature = "neural-dsp")]
    #[test]
    fn monitored_neural_pcm_matches_the_existing_unmuted_pipeline_on_the_owning_thread() {
        use zabor_native::audio_settings::{NoiseModel, ThresholdMode};
        for model in [NoiseModel::Rnnoise, NoiseModel::Deepfilter] {
            std::thread::Builder::new()
                .stack_size(16 * 1024 * 1024)
                .spawn(move || {
                    let settings = AudioSettings {
                        smart_noise_model: model,
                        mic_threshold_mode: ThresholdMode::Manual,
                        ..AudioSettings::default()
                    };
                    let (_, mut regular) = microphone(settings.clone());
                    let controls = Arc::new(AudioControls::new(settings));
                    controls.set_server_state(true, false);
                    let mixer = Arc::new(AudioMixer::new(controls.clone()));
                    let mut monitor = MicProcessor::new(controls.clone(), mixer).unwrap();
                    let mut audible = false;
                    for block in 0..80 {
                        let input: [f32; 480] = std::array::from_fn(|index| {
                            ((block * 480 + index) as f64 * std::f64::consts::TAU * 220.0 / 48000.0)
                                .sin() as f32
                                * 0.15
                        });
                        let expected = regular.process(&input, 1, false);
                        let frame = monitor.process_monitored(&input, 1, false);
                        assert_eq!(frame.pcm, [0; 480]);
                        for (sample, expected) in frame.local.unwrap().iter().zip(expected) {
                            let converted = (f64::from(*sample)
                                * if *sample < 0.0 { 32768.0 } else { 32767.0 })
                            .round() as i16;
                            assert_eq!(converted, expected, "model {model:?}, block {block}");
                            audible |= expected.unsigned_abs() > 10;
                        }
                        assert!(controls.effectively_muted());
                        assert!(!controls.speaking.load(Ordering::Acquire));
                    }
                    assert!(audible);
                })
                .unwrap()
                .join()
                .unwrap();
        }
    }
}
