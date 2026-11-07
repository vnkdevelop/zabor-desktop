use zabor_native::audio_settings::AudioSettings;
use zabor_native::smart_gate::{NoiseProcessor, SmartGate};

struct BrokenModel;

impl NoiseProcessor for BrokenModel {
    fn process(&mut self, _input: &[f32; 480], output: &mut [f32; 480]) -> bool {
        output.fill(f32::NAN);
        true
    }
    fn configure(&mut self, _strength: f32, _post_filter: f32) {}
    fn attenuation_limited(&self) -> bool {
        true
    }
}

#[test]
fn invalid_pcm_and_invalid_model_output_cannot_escape_to_the_encoder() {
    let mut gate = SmartGate::new(AudioSettings::default(), Some(Box::new(BrokenModel)), None);
    let mut output = [0.0; 480];
    let input = std::array::from_fn(|index| match index % 3 {
        0 => f32::MAX,
        1 => f32::NAN,
        _ => f32::NEG_INFINITY,
    });
    for _ in 0..100 {
        let metrics = gate.process(&input, &mut output, false);
        assert!(metrics.db.is_finite() && metrics.rms.is_finite() && metrics.peak.is_finite());
        assert!(output
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 0.999));
    }
    assert!(gate.process(&input, &mut output, false).model_failed);
}

#[test]
fn mute_discards_queued_audio_and_restarts_the_delay_with_silence() {
    let settings = AudioSettings {
        noise_suppression: false,
        speech_analyzer: false,
        ..AudioSettings::default()
    };
    let mut gate = SmartGate::new(settings, None, None);
    let mut output = [0.0; 480];
    for _ in 0..5 {
        gate.process(&[0.25; 480], &mut output, false);
    }
    assert!(output.iter().all(|sample| *sample == 0.25));
    let metrics = gate.process(&[0.25; 480], &mut output, true);
    assert!(!metrics.speaking && metrics.db == -100.0);
    assert!(output.iter().all(|sample| *sample == 0.0));
    for _ in 0..10 {
        gate.process(&[0.0; 480], &mut output, false);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }
}
