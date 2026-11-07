#![cfg(feature = "neural-dsp")]

use std::io::Write;
use std::process::{Command, Stdio};
use zabor_native::audio_settings::NoiseModel;
use zabor_native::neural::{Denoiser, Silero};

fn compare(model: NoiseModel, name: &str) {
    let frames: Vec<Vec<f32>> = (0..48)
        .map(|block| {
            (0..480)
                .map(|sample| {
                    let index = block * 480 + sample;
                    let noise = (((index as u32)
                        .wrapping_mul(1664525)
                        .wrapping_add(1013904223)
                        >> 8)
                        % 1000) as f64
                        / 1000.0
                        - 0.5;
                    let speech = if (8..40).contains(&block) {
                        (index as f64 * std::f64::consts::TAU * 220.0 / 48000.0).sin() * 0.15
                    } else {
                        0.0
                    };
                    (speech + noise * 0.02) as f32
                })
                .collect()
        })
        .collect();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/neural-reference.mjs");
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
            &serde_json::to_vec(
                &serde_json::json!({"model":name,"frames":frames,"strength":15.0,"beta":0.02}),
            )
            .unwrap(),
        )
        .unwrap();
    let reference = child.wait_with_output().unwrap();
    assert!(
        reference.status.success(),
        "reference failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference: Vec<Vec<f32>> = serde_json::from_slice(&reference.stdout).unwrap();
    assert_eq!(reference.len(), frames.len());
    let mut engine = Denoiser::new(model, 15.0, 0.02).unwrap();
    let mut absolute = 0.0_f64;
    let mut maximum = 0.0_f32;
    let mut count = 0;
    for (input, expected) in frames.iter().zip(&reference) {
        let mut output = [0.0; 480];
        engine
            .process(input.as_slice().try_into().unwrap(), &mut output)
            .unwrap();
        assert_eq!(expected.len(), 480);
        for (actual, expected) in output.iter().zip(expected) {
            let error = (actual - expected).abs();
            maximum = maximum.max(error);
            absolute += f64::from(error);
            count += 1;
        }
    }
    let mean = absolute / f64::from(count);
    eprintln!("{name} waveform difference mean={mean} max={maximum}");
    assert!(
        mean < 0.001 && maximum < 0.02,
        "{name} waveform difference mean={mean} max={maximum}"
    );
}

#[test]
fn native_rnnoise_matches_the_existing_wasm_pipeline() {
    compare(NoiseModel::Rnnoise, "rnnoise");
}

#[test]
fn native_deepfilter_reuses_the_existing_model_and_matches_wasm() {
    compare(NoiseModel::Deepfilter, "deepfilter");
}

#[test]
fn different_constants_cannot_collide_with_existing_optimizer_node_names() {
    use tract_core::prelude::*;
    let mut model = TypedModel::default();
    let vector = model
        .add_const("bias", Tensor::from_shape(&[64], &[0.0_f32; 64]).unwrap())
        .unwrap();
    let scalar = model.add_const("bias", tensor0(0.0_f32)).unwrap();
    let duplicate = model.add_const("bias", tensor0(0.0_f32)).unwrap();
    assert_eq!(scalar, duplicate);
    assert_ne!(vector, scalar);
    assert_ne!(model.node(vector.node).name, model.node(scalar.node).name);
    model.check_names().unwrap();
}

#[test]
fn rnnoise_weights_and_reference_revision_sources_keep_their_pinned_hashes() {
    use sha2::{Digest, Sha256};
    for (source, expected) in [
        (
            include_str!("../../../vendor/rnnoise2-sys-0.1.2/reference/denoise.c"),
            "a7759a51af88381d135ed90f6e73919a6f9540845e437749758f6f0dc608e4c4",
        ),
        (
            include_str!("../../../vendor/rnnoise2-sys-0.1.2/reference/rnn.c"),
            "1a72e1d568c7930e89bdfd072291930775ac842bf5373e26adb6350c3fbb0454",
        ),
    ] {
        assert_eq!(
            format!("{:x}", Sha256::digest(source.replace("\r\n", "\n"))),
            expected
        );
    }
    let model = std::fs::read(env!("RNNOISE_MODEL_PATH")).unwrap();
    assert_eq!(model.len(), 22270507);
    assert_eq!(
        format!("{:x}", Sha256::digest(model)),
        "4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43"
    );
}

#[test]
fn native_silero_resets_context_on_sequence_gaps_and_invalid_input() {
    let mut model = Silero::new().unwrap();
    let frame = std::array::from_fn(|index| {
        ((index as f64 * std::f64::consts::TAU * 220.0 / 16000.0).sin() * 0.025) as f32
    });
    let initial = model.process(&frame, 0).unwrap();
    assert!(initial.is_finite() && (0.0..=1.0).contains(&initial));
    for sequence in 1..5 {
        model.process(&frame, sequence).unwrap();
    }
    let restarted = model.process(&frame, 10).unwrap();
    assert!((initial - restarted).abs() < 1e-6);
    assert!(model.process(&[f32::NAN; 512], 11).is_err());
    assert!((model.process(&frame, 12).unwrap() - initial).abs() < 1e-6);
}

#[test]
fn native_silero_matches_the_existing_worker_across_resets_and_sequence_gaps() {
    let frames: Vec<[f32; 512]> = (0..96)
        .map(|block| {
            std::array::from_fn(|sample| {
                let index = block * 512 + sample;
                let noise = ((index as u32)
                    .wrapping_mul(1664525)
                    .wrapping_add(1013904223)
                    >> 8) as f64
                    / 16777216.0
                    - 0.5;
                let amplitude = match block % 32 {
                    0..8 => 0.0,
                    8..20 => 0.025,
                    _ => 0.3,
                };
                ((index as f64 * std::f64::consts::TAU * 220.0 / 16000.0).sin() * amplitude
                    + noise * 0.02) as f32
            })
        })
        .collect();
    let sequence = |index: usize| (index + if index >= 48 { 7 } else { 0 }) as u64;
    let request = serde_json::json!({"frames":frames.iter().enumerate().map(|(index, samples)| {
        serde_json::json!({"samples":samples.as_slice(),"sequence":sequence(index),"reset":index==72})
    }).collect::<Vec<_>>()});
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/silero-reference.cjs");
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
        .write_all(&serde_json::to_vec(&request).unwrap())
        .unwrap();
    let reference = child.wait_with_output().unwrap();
    assert!(
        reference.status.success(),
        "reference failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let probabilities: Vec<f32> = serde_json::from_slice(&reference.stdout).unwrap();
    assert_eq!(probabilities.len(), frames.len());
    let mut model = Silero::new().unwrap();
    let mut maximum = 0.0_f32;
    for (index, (frame, expected)) in frames.iter().zip(probabilities).enumerate() {
        if index == 72 {
            model.reset();
        }
        let probability = model.process(frame, sequence(index)).unwrap();
        maximum = maximum.max((probability - expected).abs());
    }
    eprintln!("silero probability difference max={maximum}");
    assert!(maximum < 1e-5, "Silero probability difference {maximum}");
}
