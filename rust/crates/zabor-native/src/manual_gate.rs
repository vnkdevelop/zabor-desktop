use crate::voice_filters::{TonalSuppressor, VoiceShaper};

pub struct ManualGate {
    delay: Vec<f32>,
    delay_index: usize,
    tone: TonalSuppressor,
    shaper: VoiceShaper,
    muted: bool,
    open: bool,
    hold: usize,
    gate_gain: f64,
    makeup_gain: f64,
    speech_peak: f64,
    speech_rms: f64,
    meter_squares: f64,
    meter_samples: usize,
    meter_db: f64,
}

impl Default for ManualGate {
    fn default() -> Self {
        Self {
            delay: vec![0.0; 2880],
            delay_index: 0,
            tone: TonalSuppressor::default(),
            shaper: VoiceShaper::default(),
            muted: false,
            open: false,
            hold: 0,
            gate_gain: 0.0,
            makeup_gain: 1.0,
            speech_peak: 0.0,
            speech_rms: 0.0,
            meter_squares: 0.0,
            meter_samples: 0,
            meter_db: -100.0,
        }
    }
}

impl ManualGate {
    pub fn meter_db(&self) -> f32 {
        self.meter_db as f32
    }
    pub fn is_speaking(&self, threshold: f32) -> bool {
        !self.muted && self.meter_db >= f64::from(threshold)
    }

    pub fn process(
        &mut self,
        input: &[f32; 128],
        output: &mut [f32; 128],
        threshold: f32,
        muted: bool,
    ) {
        if muted && !self.muted {
            self.delay.fill(0.0);
            self.gate_gain = 0.0;
            self.open = false;
            self.hold = 0;
        }
        self.muted = muted;
        let threshold = if threshold.is_finite() {
            f64::from(threshold.clamp(-60.0, -12.0))
        } else {
            -42.0
        };
        let mut mono = [0.0_f32; 128];
        let mut squares = 0.0;
        let mut peak = 0.0_f64;
        for (sample, target) in input.iter().zip(&mut mono) {
            let sample = self.tone.process(
                if sample.is_finite() {
                    f64::from(*sample)
                } else {
                    0.0
                },
                !self.open,
            );
            *target = sample as f32;
            squares += sample * sample;
            peak = peak.max(sample.abs());
        }
        self.meter_squares += squares;
        self.meter_samples += 128;
        if self.meter_samples >= 2400 {
            let measured = (20.0
                * (self.meter_squares / self.meter_samples as f64)
                    .sqrt()
                    .max(1e-6)
                    .log10())
            .clamp(-100.0, 0.0);
            self.meter_db +=
                (measured - self.meter_db) * if measured > self.meter_db { 0.55 } else { 0.18 };
            self.meter_samples = 0;
            self.meter_squares = 0.0;
        }
        let rms = (squares / 128.0).sqrt();
        let transient = peak / rms.max(1e-6) > 3.8;
        let db = 20.0 * rms.max(1e-6).log10();
        if muted {
            self.open = false;
            self.hold = 0;
        } else if db >= threshold && !transient {
            self.open = true;
            self.hold = 18000;
        } else if self.open {
            if db >= threshold - 8.0 {
                self.hold = 18000;
            } else {
                self.hold = self.hold.saturating_sub(128);
                if self.hold == 0 {
                    self.open = false;
                }
            }
        }
        let target = if self.open { 1.0 } else { 0.0 };
        let makeup = if self.speech_peak > 0.0015 {
            (0.891 / self.speech_peak).clamp(1.0, 4.5)
        } else {
            4.5
        };
        let coefficient = if makeup > self.makeup_gain {
            0.000004
        } else {
            0.0004
        };
        self.shaper.set_speech_reference(self.speech_rms);
        let mut shaped_peak = 0.0_f64;
        let mut shaped_squares = 0.0;
        for (sample, output) in mono.iter().zip(output) {
            let delayed = self.delay[self.delay_index];
            self.delay[self.delay_index] = *sample;
            self.delay_index = (self.delay_index + 1) % self.delay.len();
            self.gate_gain += (target - self.gate_gain)
                * if target > self.gate_gain {
                    1.0 / 48.0
                } else {
                    1.0 / 5760.0
                };
            self.makeup_gain += (makeup - self.makeup_gain) * coefficient;
            let voiced = self.shaper.process(f64::from(delayed));
            shaped_peak = shaped_peak.max(voiced.abs());
            shaped_squares += voiced * voiced;
            *output = if muted {
                0.0
            } else {
                (voiced * self.gate_gain * self.makeup_gain).clamp(-0.999, 0.999) as f32
            };
        }
        if self.open && shaped_peak > 0.0015 {
            self.speech_peak = if shaped_peak > self.speech_peak {
                shaped_peak
            } else {
                self.speech_peak + (shaped_peak - self.speech_peak) * 0.05
            };
            let rms = (shaped_squares / 128.0).sqrt();
            self.speech_rms = if self.speech_rms > 0.0 {
                self.speech_rms + (rms - self.speech_rms) * 0.05
            } else {
                rms
            };
        }
    }
}
