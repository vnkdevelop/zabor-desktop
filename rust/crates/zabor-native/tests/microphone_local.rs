#![cfg(feature = "native-media")]

use futures_util::StreamExt;
use libwebrtc::audio_stream::native::NativeAudioStream;
use libwebrtc::peer_connection_factory::{native::PeerConnectionFactoryExt, PeerConnectionFactory};
use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use zabor_native::audio_mix::{AudioControls, AudioMixer};
use zabor_native::audio_settings::AudioSettings;
use zabor_native::microphone::{audio_source, MicProcessor, MicSubmission};

fn processor(settings: AudioSettings) -> (Arc<AudioControls>, Arc<AudioMixer>, MicProcessor) {
    let controls = Arc::new(AudioControls::new(settings));
    let mixer = Arc::new(AudioMixer::new(controls.clone()));
    let processor = MicProcessor::new(controls.clone(), mixer.clone()).unwrap();
    (controls, mixer, processor)
}

#[test]
fn raw_mono_stereo_gain_and_ultra_low_latency_keep_the_reference_branches() {
    let (controls, _, mut mic) = processor(AudioSettings {
        noise_suppression: false,
        ..AudioSettings::default()
    });
    let mono = mic.process(&[0.1; 480], 1, false);
    assert!(mono.iter().all(|sample| sample.abs_diff(5570) <= 1));
    let stereo: Vec<f32> = (0..480).flat_map(|_| [0.0, 0.2]).collect();
    assert_eq!(mic.process(&stereo, 2, false), mono);
    let settings = AudioSettings {
        noise_suppression: false,
        input_volume: 50.0,
        ..AudioSettings::default()
    };
    assert!(controls.apply_settings(settings));
    assert!(mic
        .process(&[0.1; 480], 1, false)
        .iter()
        .all(|sample| sample.abs_diff(2785) <= 1));
    controls.apply_settings(AudioSettings {
        ultra_low_latency: true,
        input_volume: 0.0,
        ..AudioSettings::default()
    });
    assert!(mic
        .process(&[0.1; 480], 1, false)
        .iter()
        .all(|sample| sample.abs_diff(3277) <= 1));
    assert!(!controls.processor_failed.load(Ordering::Acquire));
}

#[test]
fn invalid_device_frames_fail_closed_and_a_discontinuity_resets_the_failure() {
    let (controls, _, mut mic) = processor(AudioSettings {
        noise_suppression: false,
        ..AudioSettings::default()
    });
    for (samples, channels) in [
        (vec![f32::NAN; 480], 1),
        (vec![0.1; 479], 1),
        (vec![0.1; 480], 0),
    ] {
        assert_eq!(mic.process(&samples, channels, false), [0; 480]);
        assert!(controls.processor_failed.load(Ordering::Acquire));
        assert_eq!(mic.process(&[0.1; 480], 1, false), [0; 480]);
        assert!(mic
            .process(&[0.1; 480], 1, true)
            .iter()
            .any(|sample| *sample != 0));
        assert!(!controls.processor_failed.load(Ordering::Acquire));
    }
}

#[test]
fn user_and_server_restrictions_independently_silence_the_capture_path() {
    let (controls, _, mut mic) = processor(AudioSettings {
        noise_suppression: false,
        ..AudioSettings::default()
    });
    for state in [(true, false), (false, true), (true, true)] {
        controls.set_user_state(state.0, state.1);
        assert_eq!(mic.process(&[0.2; 480], 1, false), [0; 480]);
        assert!(!controls.speaking.load(Ordering::Acquire));
        assert_eq!(
            f32::from_bits(controls.mic_db.load(Ordering::Acquire)),
            -100.0
        );
    }
    controls.set_user_state(false, false);
    controls.set_server_state(true, false);
    assert_eq!(mic.process(&[0.2; 480], 1, false), [0; 480]);
    controls.set_user_state(false, false);
    assert!(controls.effectively_muted());
    controls.set_server_state(false, true);
    assert_eq!(mic.process(&[0.2; 480], 1, false), [0; 480]);
    controls.set_server_state(false, false);
    assert!(mic
        .process(&[0.2; 480], 1, false)
        .iter()
        .any(|sample| *sample != 0));
}

#[tokio::test]
async fn final_submission_rechecks_mute_epochs_and_cancellation_without_a_device() {
    let (controls, _, mut mic) = processor(AudioSettings {
        noise_suppression: false,
        ..AudioSettings::default()
    });
    let source = audio_source();
    let factory = PeerConnectionFactory::default();
    let track = factory.create_audio_track("local-fixture", source.clone());
    let mut output = NativeAudioStream::new(track, 48000, 1);
    let stop = CancellationToken::new();
    let submit = MicSubmission::new(source, controls.clone(), stop.clone());
    let mut pcm = mic.process(&[0.1; 480], 1, false);
    controls.set_user_state(true, false);
    controls.set_user_state(false, false);
    submit.submit(&mut pcm, mic.epoch()).unwrap();
    assert_eq!(pcm, [0; 480]);
    assert!(tokio::time::timeout(Duration::from_secs(1), output.next())
        .await
        .unwrap()
        .unwrap()
        .data
        .iter()
        .all(|sample| *sample == 0));
    let mut pcm = mic.process(&[0.1; 480], 1, false);
    submit.submit(&mut pcm, mic.epoch()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(1), output.next())
        .await
        .unwrap()
        .unwrap()
        .data
        .iter()
        .any(|sample| *sample != 0));
    stop.cancel();
    let mut pcm = mic.process(&[0.1; 480], 1, false);
    submit.submit(&mut pcm, mic.epoch()).unwrap();
    assert_eq!(pcm, [0; 480]);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), output.next())
            .await
            .is_err()
    );
    output.close();
}

#[test]
fn aec_consumes_only_committed_bounded_reverse_frames_and_never_emits_nonfinite_pcm() {
    let (controls, mixer, mut mic) = processor(AudioSettings {
        noise_suppression: false,
        echo_cancellation: true,
        ..AudioSettings::default()
    });
    for _ in 0..30 {
        mixer.played(&[0.1; 960]);
    }
    assert_eq!(mixer.reverse.lock().unwrap().len(), 4800);
    let _ = mic.process(&[0.05; 480], 1, false);
    assert!(mixer.reverse.lock().unwrap().is_empty());
    assert!(!controls.processor_failed.load(Ordering::Acquire));
    controls.set_user_state(true, false);
    assert_eq!(mic.process(&[0.05; 480], 1, false), [0; 480]);
}

#[cfg(feature = "neural-dsp")]
#[test]
fn neural_models_stay_on_the_owner_thread_and_clear_speech_after_mute_and_gaps() {
    use zabor_native::audio_settings::{NoiseModel, ThresholdMode};
    for model in [NoiseModel::Rnnoise, NoiseModel::Deepfilter] {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let (controls, _, mut mic) = processor(AudioSettings {
                    smart_noise_model: model,
                    mic_threshold_mode: ThresholdMode::Manual,
                    speech_analyzer: true,
                    ..AudioSettings::default()
                });
                let mut audible = false;
                for block in 0..80 {
                    let input: [f32; 480] = std::array::from_fn(|index| {
                        ((block * 480 + index) as f64 * std::f64::consts::TAU * 220.0 / 48000.0)
                            .sin() as f32
                            * 0.15
                    });
                    audible |= mic
                        .process(&input, 1, false)
                        .iter()
                        .any(|sample| sample.unsigned_abs() > 10);
                }
                assert!(audible, "model {model:?} never reached the media output");
                assert!(!controls.processor_failed.load(Ordering::Acquire));
                controls.set_user_state(true, false);
                controls.set_user_state(false, false);
                for _ in 0..40 {
                    assert_eq!(mic.process(&[0.0; 480], 1, false), [0; 480]);
                }
                assert_eq!(mic.process(&[0.1; 480], 1, true), [0; 480]);
                assert!(!controls.speaking.load(Ordering::Acquire));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

#[cfg(all(windows, feature = "native-media"))]
#[tokio::test]
async fn cancelled_audio_start_never_opens_physical_devices() {
    let stop = CancellationToken::new();
    stop.cancel();
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    assert!(
        zabor_native::native_audio::NativeAudio::start(controls, stop)
            .await
            .is_err()
    );
}

#[cfg(not(feature = "neural-dsp"))]
#[test]
fn missing_neural_features_cannot_silently_replace_noise_suppression_with_raw_audio() {
    let controls = Arc::new(AudioControls::new(AudioSettings::default()));
    let mixer = Arc::new(AudioMixer::new(controls.clone()));
    assert!(MicProcessor::new(controls, mixer).is_err());
}
