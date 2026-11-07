use webrtc_vad::{SampleRate, Vad, VadMode};

pub struct VoiceActivity {
    detector: Vad,
    pcm: [i16; 480],
    hangover: u32,
}

impl Default for VoiceActivity {
    fn default() -> Self {
        Self {
            detector: Vad::new_with_rate_and_mode(SampleRate::Rate48kHz, VadMode::Aggressive),
            pcm: [0; 480],
            hangover: 0,
        }
    }
}

impl VoiceActivity {
    pub fn reset(&mut self) {
        self.detector.reset();
        self.detector.set_sample_rate(SampleRate::Rate48kHz);
        self.detector.set_mode(VadMode::Aggressive);
        self.pcm.fill(0);
        self.hangover = 0;
    }

    pub fn process(&mut self, frame: &[f32; 480]) -> bool {
        for (target, sample) in self.pcm.iter_mut().zip(frame) {
            let sample = if sample.is_finite() {
                f64::from(sample.clamp(-1.0, 1.0))
            } else {
                0.0
            };
            let scaled = sample * if sample < 0.0 { 32768.0 } else { 32767.0 };
            *target = (scaled + 0.5).floor() as i16;
        }
        if self.detector.is_voice_segment(&self.pcm) == Ok(true) {
            self.hangover = 15;
            true
        } else if self.hangover > 0 {
            self.hangover -= 1;
            true
        } else {
            false
        }
    }
}
