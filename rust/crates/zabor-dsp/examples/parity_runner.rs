use std::io::{self, Read};
use zabor_dsp::{Decimator48To16, VoicingEstimator};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(4 * 1_000_000 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1_000_000 || bytes.len() < 49 * 4 || bytes.len() % 4 != 0 {
        return Err("invalid fixture".into());
    }
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err("non-finite fixture".into());
    }
    let taps: [f32; 49] = samples[..49].try_into()?;
    let mut decimator = Decimator48To16::new(taps);
    let mut pitch = VoicingEstimator::default();
    let mut output = Vec::new();
    let mut block = [0.0_f64; 43];
    for chunk in samples[49..].chunks(128) {
        let written = decimator
            .process_block(chunk, &mut block)
            .map_err(|_| "output capacity")?;
        output.extend_from_slice(&block[..written]);
    }
    let mut scores = Vec::new();
    for chunk in output.chunks(512) {
        let rounded: Vec<f32> = chunk.iter().map(|&sample| sample as f32).collect();
        pitch.push_block(&rounded);
        scores.push(pitch.measure());
    }
    print!("{{\"decimated\":[");
    for (i, sample) in output.iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        print!("{sample:.17}");
    }
    print!("],\"voicing\":[");
    for (i, score) in scores.iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        print!("{{\"score\":{:.17},\"lag\":{}}}", score.score, score.lag);
    }
    println!("]}}");
    Ok(())
}
