const TAPS: usize = 49;
const PITCH_SIZE: usize = 512;
const WINDOW: usize = 256;
const MIN_LAG: usize = 40;
const MAX_LAG: usize = 229;
const SPAN: usize = WINDOW + MAX_LAG + 1;

#[derive(Clone)]
pub struct Decimator48To16 {
    taps: [f32; TAPS],
    history: [f32; TAPS],
    write_index: usize,
    phase: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputTooSmall;

impl Decimator48To16 {
    pub fn new(taps: [f32; TAPS]) -> Self {
        Self {
            taps,
            history: [0.0; TAPS],
            write_index: 0,
            phase: 0,
        }
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.write_index = 0;
        self.phase = 0;
    }

    pub fn process_block(
        &mut self,
        input: &[f32],
        output: &mut [f64],
    ) -> Result<usize, OutputTooSmall> {
        let required = input.len() / 3 + (input.len() % 3 + self.phase) / 3;
        if output.len() < required {
            return Err(OutputTooSmall);
        }
        let mut written = 0;
        for &sample in input {
            self.history[self.write_index] = sample;
            self.write_index = (self.write_index + 1) % TAPS;
            self.phase += 1;
            if self.phase < 3 {
                continue;
            }
            self.phase = 0;
            let mut filtered = 0.0_f64;
            let mut index = (self.write_index + TAPS - 1) % TAPS;
            for &tap in &self.taps {
                filtered += f64::from(tap) * f64::from(self.history[index]);
                index = (index + TAPS - 1) % TAPS;
            }
            output[written] = filtered.clamp(-1.0, 1.0);
            written += 1;
        }
        Ok(written)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voicing {
    pub score: f64,
    pub lag: usize,
}

#[derive(Clone)]
pub struct VoicingEstimator {
    buffer: [f32; PITCH_SIZE],
    scratch: [f32; PITCH_SIZE],
    write_index: usize,
    filled: usize,
}

impl Default for VoicingEstimator {
    fn default() -> Self {
        Self {
            buffer: [0.0; PITCH_SIZE],
            scratch: [0.0; PITCH_SIZE],
            write_index: 0,
            filled: 0,
        }
    }
}

impl VoicingEstimator {
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.scratch.fill(0.0);
        self.write_index = 0;
        self.filled = 0;
    }

    pub fn push_block(&mut self, input: &[f32]) {
        for &sample in input {
            self.buffer[self.write_index] = sample;
            self.write_index = (self.write_index + 1) % PITCH_SIZE;
        }
        self.filled = self.filled.saturating_add(input.len()).min(PITCH_SIZE);
    }

    pub fn measure(&mut self) -> Voicing {
        let empty = Voicing { score: 0.0, lag: 0 };
        if self.filled < SPAN {
            return empty;
        }
        let mut index = (self.write_index + PITCH_SIZE - 1) % PITCH_SIZE;
        for sample in &mut self.scratch[..SPAN] {
            *sample = self.buffer[index];
            index = (index + PITCH_SIZE - 1) % PITCH_SIZE;
        }
        let mut frame_energy = 0.0_f64;
        for &sample in &self.scratch[..WINDOW] {
            frame_energy += f64::from(sample) * f64::from(sample);
        }
        if frame_energy < 1e-9 {
            return empty;
        }
        let mut lag_energy = 0.0_f64;
        for &sample in &self.scratch[MIN_LAG..MIN_LAG + WINDOW] {
            lag_energy += f64::from(sample) * f64::from(sample);
        }
        let mut best = empty;
        for lag in MIN_LAG..=MAX_LAG {
            if lag_energy > 1e-9 {
                let mut correlation = 0.0_f64;
                for i in 0..WINDOW {
                    correlation += f64::from(self.scratch[i]) * f64::from(self.scratch[i + lag]);
                }
                let score = correlation / (frame_energy * lag_energy).sqrt();
                if score > best.score {
                    best = Voicing { score, lag };
                }
            }
            let entering = f64::from(self.scratch[lag + WINDOW]);
            let leaving = f64::from(self.scratch[lag]);
            lag_energy += entering * entering - leaving * leaving;
        }
        best.score = best.score.clamp(0.0, 1.0);
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insufficient_output_does_not_advance_decimator_state() {
        let mut taps = [0.0; TAPS];
        taps[0] = 1.0;
        let mut filter = Decimator48To16::new(taps);
        assert_eq!(
            filter.process_block(&[0.1, 0.2, 0.3], &mut []),
            Err(OutputTooSmall)
        );
        let mut output = [0.0];
        assert_eq!(filter.process_block(&[0.1, 0.2, 0.3], &mut output), Ok(1));
        assert_eq!(output[0], f64::from(0.3_f32));
    }

    #[test]
    fn reset_restores_silence_and_decimation_phase() {
        let mut filter = Decimator48To16::new([1.0; TAPS]);
        filter.process_block(&[1.0, 1.0], &mut []).unwrap();
        filter.reset();
        assert_eq!(filter.process_block(&[0.0, 0.0], &mut []), Ok(0));
        let mut output = [1.0];
        filter.process_block(&[0.0], &mut output).unwrap();
        assert_eq!(output[0], 0.0);
        let mut pitch = VoicingEstimator::default();
        pitch.push_block(&[0.5; PITCH_SIZE]);
        pitch.reset();
        assert_eq!(pitch.measure(), Voicing { score: 0.0, lag: 0 });
    }
}
