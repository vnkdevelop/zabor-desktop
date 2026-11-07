use std::io::Write;
use std::process::{Command, Stdio};
use zabor_native::voice_activity::VoiceActivity;

#[test]
fn native_webrtc_vad_and_hangover_match_the_existing_wasm_after_resets() {
    let frames: Vec<[f32; 480]> = (0..240)
        .map(|block| {
            std::array::from_fn(|sample| {
                let index = block * 480 + sample;
                if (20..90).contains(&block) || (130..180).contains(&block) {
                    let time = index as f64 / 48000.0;
                    ((time * std::f64::consts::TAU * 160.0).sin() * 0.08
                        + (time * std::f64::consts::TAU * 720.0).sin() * 0.03)
                        as f32
                } else {
                    0.0
                }
            })
        })
        .collect();
    let resets = [0, 140, 220];
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vad-reference.cjs");
    let mut child = Command::new("cmd.exe")
        .args(["/d", "/c", "node"])
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let payload = serde_json::json!({"frames":frames.iter().map(|frame| frame.as_slice()).collect::<Vec<_>>(),"resets":resets});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&payload).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<bool> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(expected.len(), frames.len());
    assert!(expected.iter().any(|value| *value) && expected.iter().any(|value| !value));
    let mut detector = VoiceActivity::default();
    for (index, (frame, expected)) in frames.iter().zip(expected).enumerate() {
        if resets.contains(&index) {
            detector.reset();
        }
        assert_eq!(
            detector.process(frame),
            expected,
            "VAD differed on frame {index}"
        );
    }
}
