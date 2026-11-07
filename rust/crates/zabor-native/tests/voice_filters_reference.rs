use base64::Engine;
use std::io::Write;
use std::process::{Command, Stdio};
use zabor_native::audio_settings::{AudioSettings, ThresholdMode};
use zabor_native::manual_gate::ManualGate;
use zabor_native::smart_gate::{NoiseProcessor, SmartGate, VoiceProbability};
use zabor_native::voice_filters::{TonalSuppressor, VoiceShaper};

fn samples(manual: bool) -> Vec<f32> {
    (0..384000)
        .map(|index| {
            let block = index / 128;
            if manual && (2500..2750).contains(&block) {
                return 0.0;
            }
            let time = index as f64 / 48000.0;
            let hum = (time * std::f64::consts::TAU * 50.0).sin() * 0.02;
            let whine = (time * std::f64::consts::TAU * 2200.0).sin() * 0.01;
            let speech = if index > 120000 {
                (time * std::f64::consts::TAU * 220.0).sin() * 0.08
            } else {
                0.0
            };
            (hum + whine + speech) as f32
        })
        .collect()
}

fn reference(kind: &str, samples: &[f32]) -> Vec<f32> {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/voice-reference.cjs");
    let mut child = Command::new("cmd.exe")
        .args(["/d", "/c", "node"])
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({"kind":kind,"samples":samples})).unwrap(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(output.stdout)
        .unwrap();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

fn compare(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    let mut maximum = 0.0_f32;
    let mut total = 0.0_f64;
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(actual.is_finite());
        let error = (actual - expected).abs();
        maximum = maximum.max(error);
        total += f64::from(error);
    }
    let mean = total / actual.len() as f64;
    assert!(
        mean < 1e-6 && maximum < 2e-4,
        "voice shaping difference: mean={mean}, max={maximum}"
    );
}

#[test]
fn native_tonal_notches_equalizer_and_deesser_match_the_current_typescript() {
    let samples = samples(false);
    let expected = reference("filters", &samples);
    let mut tone = TonalSuppressor::default();
    let mut shaper = VoiceShaper::default();
    shaper.set_speech_reference(0.05);
    let actual: Vec<_> = samples
        .iter()
        .map(|sample| shaper.process(tone.process(f64::from(*sample), true)) as f32)
        .collect();
    compare(&actual, &expected);
}

#[test]
fn native_manual_gate_matches_threshold_hold_lookahead_and_makeup() {
    let samples = samples(true);
    let expected = reference("manual", &samples);
    let mut gate = ManualGate::default();
    let mut actual = vec![0.0_f32; samples.len()];
    for (index, (input, output)) in samples
        .as_chunks::<128>()
        .0
        .iter()
        .zip(actual.as_chunks_mut::<128>().0.iter_mut())
        .enumerate()
    {
        gate.process(input, output, -30.0, (2500..2750).contains(&index));
    }
    compare(&actual, &expected);
    assert!(actual[2500 * 128..2750 * 128]
        .iter()
        .all(|sample| *sample == 0.0));
}

struct Passthrough;

impl NoiseProcessor for Passthrough {
    fn process(&mut self, input: &[f32; 480], output: &mut [f32; 480]) -> bool {
        *output = *input;
        true
    }
    fn configure(&mut self, _strength: f32, _post_filter: f32) {}
    fn attenuation_limited(&self) -> bool {
        false
    }
}

fn compare_smart(manual: bool) {
    let samples = samples(false);
    let expected = reference(if manual { "smart-manual" } else { "smart-auto" }, &samples);
    let settings = AudioSettings {
        speech_analyzer: false,
        mic_threshold_mode: if manual {
            ThresholdMode::Manual
        } else {
            ThresholdMode::Auto
        },
        manual_threshold_value: -30.0,
        ..AudioSettings::default()
    };
    let mut gate = SmartGate::new(settings, Some(Box::new(Passthrough)), None);
    let mut actual = vec![0.0; samples.len()];
    for (input, output) in samples
        .as_chunks::<480>()
        .0
        .iter()
        .zip(actual.as_chunks_mut::<480>().0)
    {
        gate.process(input, output, false);
    }
    compare(&actual, &expected);
}

#[test]
fn native_smart_gate_model_input_gain_and_alc_match_typescript() {
    compare_smart(false);
}

#[test]
fn native_smart_gate_manual_decisions_and_lookahead_match_typescript() {
    compare_smart(true);
}

struct Probabilities;

impl VoiceProbability for Probabilities {
    fn probability(&mut self, _input: &[f32; 512], sequence: u64) -> Option<f32> {
        Some(if (70..140).contains(&sequence) {
            0.8
        } else if (180..205).contains(&sequence) {
            0.5
        } else {
            0.05
        })
    }
}

#[test]
fn native_smart_gate_voicing_attack_release_and_preroll_match_typescript() {
    let samples = samples(false);
    let expected = reference("smart-vad", &samples);
    let mut gate = SmartGate::new(
        AudioSettings::default(),
        Some(Box::new(Passthrough)),
        Some(Box::new(Probabilities)),
    );
    let mut actual = vec![0.0; samples.len()];
    for (input, output) in samples
        .as_chunks::<480>()
        .0
        .iter()
        .zip(actual.as_chunks_mut::<480>().0)
    {
        gate.process(input, output, false);
    }
    compare(&actual, &expected);
}
