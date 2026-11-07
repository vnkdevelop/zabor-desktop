use crate::audio_settings::{AudioSettings, ThresholdMode};
use crate::voice_filters::{TonalSuppressor, VoiceShaper};
use std::f64::consts::{PI, TAU};
use zabor_dsp::{Decimator48To16, VoicingEstimator};

pub trait NoiseProcessor {
    fn process(&mut self, input: &[f32; 480], output: &mut [f32; 480]) -> bool;
    fn configure(&mut self, strength: f32, post_filter: f32);
    fn attenuation_limited(&self) -> bool;
}

pub trait VoiceProbability {
    fn probability(&mut self, input: &[f32; 512], sequence: u64) -> Option<f32>;
}

#[derive(Clone, Copy, Default)]
pub struct MicMetrics {
    pub db: f32,
    pub speaking: bool,
    pub rms: f32,
    pub peak: f32,
    pub probability: f32,
    pub zero_crossings: f32,
    pub spectral_tilt: f32,
    pub model_failed: bool,
}

struct SpeechFrame {
    samples: [f32; 480],
    id: i64,
    rms: f32,
    speech: bool,
    transient: bool,
}

pub struct SmartGate {
    settings: AudioSettings,
    noise: Option<Box<dyn NoiseProcessor>>,
    analyzer: Option<Box<dyn VoiceProbability>>,
    tone: TonalSuppressor,
    shaper: VoiceShaper,
    decimator: Decimator48To16,
    voicing: VoicingEstimator,
    vad: [f32; 512],
    vad_fill: usize,
    vad_sequence: u64,
    previous_lag: usize,
    voiced_hold: u32,
    frames: Vec<SpeechFrame>,
    frame_id: i64,
    write: usize,
    buffered: u64,
    segment_open: bool,
    gate_open: bool,
    segment_peak: f64,
    attack: u32,
    attack_gap: u32,
    first_attack: i64,
    silence: u32,
    manual_hold: u32,
    previous_rms: f64,
    rms_smoothed: f64,
    noise_high: f64,
    noise_history: [f32; 200],
    noise_count: usize,
    closed_frames: u32,
    transient_candidate: Option<(i64, f64, u32)>,
    transient_hold: u32,
    gate_gain: f64,
    gate_hold: u32,
    engine_peak: f64,
    engine_rms: f64,
    engine_frames: u32,
    engine_gain: f64,
    engine_applied: f64,
    alc_peak: f64,
    alc_rms: f64,
    alc_frames: u32,
    alc_gain: f64,
    alc_applied: f64,
    veto_frames: u32,
    veto_latched: bool,
    output_delay: [[f32; 480]; 2],
    output_index: usize,
    metrics: MicMetrics,
    was_muted: bool,
}

impl SmartGate {
    pub fn new(
        settings: AudioSettings,
        noise: Option<Box<dyn NoiseProcessor>>,
        analyzer: Option<Box<dyn VoiceProbability>>,
    ) -> Self {
        let mut taps = [0.0_f32; 49];
        let mut sum = 0.0;
        for (index, tap) in taps.iter_mut().enumerate() {
            let offset = index as f64 - 24.0;
            let sinc = if offset == 0.0 {
                0.3
            } else {
                (TAU * 0.15 * offset).sin() / (PI * offset)
            };
            let value = sinc * (0.54 - 0.46 * (TAU * index as f64 / 48.0).cos());
            *tap = value as f32;
            sum += value;
        }
        for tap in &mut taps {
            *tap = (f64::from(*tap) / sum) as f32;
        }
        Self {
            settings,
            noise,
            analyzer,
            tone: TonalSuppressor::default(),
            shaper: VoiceShaper::default(),
            decimator: Decimator48To16::new(taps),
            voicing: VoicingEstimator::default(),
            vad: [0.0; 512],
            vad_fill: 0,
            vad_sequence: 0,
            previous_lag: 0,
            voiced_hold: 0,
            frames: (0..25)
                .map(|_| SpeechFrame {
                    samples: [0.0; 480],
                    id: -1,
                    rms: 0.0,
                    speech: false,
                    transient: false,
                })
                .collect(),
            frame_id: 0,
            write: 0,
            buffered: 0,
            segment_open: false,
            gate_open: false,
            segment_peak: 0.0,
            attack: 0,
            attack_gap: 0,
            first_attack: -1,
            silence: 0,
            manual_hold: 0,
            previous_rms: 0.0,
            rms_smoothed: 0.0,
            noise_high: 0.003,
            noise_history: [0.0; 200],
            noise_count: 0,
            closed_frames: 0,
            transient_candidate: None,
            transient_hold: 0,
            gate_gain: 0.0,
            gate_hold: 0,
            engine_peak: 0.0,
            engine_rms: 0.0,
            engine_frames: 0,
            engine_gain: 1.0,
            engine_applied: 1.0,
            alc_peak: 0.0,
            alc_rms: 0.0,
            alc_frames: 0,
            alc_gain: 1.0,
            alc_applied: 1.0,
            veto_frames: 0,
            veto_latched: false,
            output_delay: [[0.0; 480]; 2],
            output_index: 0,
            metrics: MicMetrics {
                db: -100.0,
                ..MicMetrics::default()
            },
            was_muted: false,
        }
    }

    pub fn configure_level(&mut self, volume: f32, threshold: f32, strength: f32) {
        if !volume.is_finite() || !threshold.is_finite() || !strength.is_finite() {
            return;
        }
        self.settings.input_volume = volume.clamp(0.0, 200.0);
        self.settings.manual_threshold_value = threshold.clamp(-60.0, -12.0);
        self.settings.suppression_strength = strength.clamp(5.0, 30.0);
        let beta = self.settings.post_filter_beta();
        if let Some(noise) = &mut self.noise {
            noise.configure(self.settings.suppression_strength, beta);
        }
    }

    fn clear_gate(&mut self) {
        for frame in &mut self.frames {
            frame.samples.fill(0.0);
            frame.id = -1;
            frame.speech = false;
            frame.transient = false;
        }
        self.output_delay = [[0.0; 480]; 2];
        self.segment_open = false;
        self.gate_open = false;
        self.segment_peak = 0.0;
        self.attack = 0;
        self.attack_gap = 0;
        self.silence = 0;
        self.first_attack = -1;
        self.manual_hold = 0;
        self.gate_gain = 0.0;
        self.gate_hold = 0;
        self.buffered = 0;
        self.vad.fill(0.0);
        self.vad_fill = 0;
        self.vad_sequence = self.vad_sequence.saturating_add(2);
        self.voicing.reset();
        self.decimator.reset();
        self.previous_lag = 0;
        self.voiced_hold = 0;
        self.transient_candidate = None;
        self.transient_hold = 0;
        self.rms_smoothed = 0.0;
        self.previous_rms = 0.0;
        self.closed_frames = 0;
        self.veto_frames = 0;
        self.veto_latched = false;
        self.write = 0;
        self.output_index = 0;
        self.tone = TonalSuppressor::default();
    }

    fn mark_speech(&mut self, first: i64) {
        for frame in &mut self.frames {
            if frame.id >= first && !frame.transient {
                frame.speech = true;
            }
        }
    }

    fn probability(&mut self, probability: f32, voiced: bool, end: i64) {
        let probability = f64::from(probability);
        self.metrics.probability = probability as f32;
        let threshold = if voiced {
            0.4
        } else if self.voiced_hold > 0 {
            0.46
        } else {
            0.54
        };
        if probability > self.segment_peak && (self.segment_open || probability >= 0.4) {
            self.segment_peak = probability;
        }
        if probability >= threshold {
            let transient = self
                .frames
                .iter()
                .any(|frame| frame.id >= end - 4 && frame.id <= end && frame.transient);
            if !self.segment_open && transient {
                self.attack = 0;
                self.attack_gap = 0;
                self.first_attack = -1;
                self.silence = 0;
            } else {
                if self.attack == 0 {
                    self.first_attack = end;
                }
                self.attack = self.attack.saturating_add(1);
                self.attack_gap = 0;
                self.silence = 0;
                if !self.segment_open && self.attack >= 2 {
                    self.segment_open = true;
                    self.mark_speech(self.first_attack - 4 - 14);
                    self.first_attack = -1;
                }
            }
        } else if self.segment_open {
            self.attack = 0;
            self.attack_gap = 0;
            self.first_attack = -1;
            if probability >= 0.16 {
                self.silence = 0;
            } else {
                self.silence += 1;
                let release =
                    5 + (7.0 * ((self.segment_peak - 0.4) / 0.5).clamp(0.0, 1.0)).round() as u32;
                if self.silence >= release {
                    self.segment_open = false;
                    self.silence = 0;
                    self.segment_peak = 0.0;
                }
            }
        } else if self.attack > 0 {
            self.attack_gap += 1;
            if probability < 0.16 || self.attack_gap > 3 {
                self.attack = 0;
                self.attack_gap = 0;
                self.first_attack = -1;
            }
        }
    }

    pub fn process(
        &mut self,
        input: &[f32; 480],
        output: &mut [f32; 480],
        muted: bool,
    ) -> MicMetrics {
        if muted != self.was_muted {
            self.clear_gate();
            self.was_muted = muted;
        }
        if muted {
            output.fill(0.0);
            self.metrics.speaking = false;
            self.metrics.db = -100.0;
            return self.metrics;
        }
        let noise_enabled = self.settings.noise_suppression;
        let analyzer_enabled = self.analyzer.is_some();
        let adapting =
            noise_enabled && !self.gate_open && self.previous_rms <= self.noise_high * 2.0;
        let mut frame = [0.0_f32; 480];
        for (from, to) in input.iter().zip(&mut frame) {
            let sample = if from.is_finite() {
                f64::from(from.clamp(-1.0, 1.0))
            } else {
                0.0
            };
            *to = if noise_enabled {
                self.tone.process(sample, adapting) as f32
            } else {
                sample as f32
            };
        }
        let mut squares = 0.0;
        let mut peak = 0.0_f64;
        let mut low = 0.0;
        let mut low_squares = 0.0;
        let mut high_squares = 0.0;
        let mut crossings = 0;
        for (index, sample) in frame.iter().enumerate() {
            let sample = f64::from(*sample);
            squares += sample * sample;
            peak = peak.max(sample.abs());
            low += 0.12 * (sample - low);
            low_squares += low * low;
            high_squares += (sample - low).powi(2);
            if index > 0 && (sample >= 0.0) != (frame[index - 1] >= 0.0) {
                crossings += 1;
            }
        }
        let rms = (squares / 480.0).sqrt();
        let rate = if rms > self.rms_smoothed { 0.6 } else { 0.2 };
        self.rms_smoothed = rate * rms + (1.0 - rate) * self.rms_smoothed;
        self.metrics.db = (20.0 * self.rms_smoothed.max(1e-6).log10()).clamp(-100.0, 0.0) as f32;
        self.metrics.rms = rms as f32;
        self.metrics.peak = peak as f32;
        self.metrics.zero_crossings = crossings as f32 / 479.0;
        self.metrics.spectral_tilt = (high_squares / low_squares.max(1e-12)).sqrt() as f32;
        self.transient_hold = self.transient_hold.saturating_sub(1);
        let nominated = 20.0 * (rms / self.previous_rms.max(1e-6)).log10() >= 15.0
            && peak / rms.max(1e-6) >= 6.0
            && rms >= (self.noise_high * 2.0).max(0.0006);
        self.previous_rms = rms;
        let mut convicted = false;
        if let Some((first, mut highest, mut age)) = self.transient_candidate {
            highest = highest.max(rms);
            age += 1;
            if rms <= highest * 0.25 {
                for frame in &mut self.frames {
                    if frame.id >= first {
                        frame.transient = true;
                    }
                }
                self.transient_candidate = None;
                self.transient_hold = 5;
                convicted = true;
            } else if age >= 6 {
                self.transient_candidate = None;
            } else {
                self.transient_candidate = Some((first, highest, age));
            }
        } else if nominated {
            self.transient_candidate = Some((self.frame_id, rms, 0));
        }
        let transient = convicted || self.transient_hold > 0;
        let in_transient = transient || self.transient_candidate.is_some();
        if self.gate_open {
            self.closed_frames = 0;
        } else {
            self.closed_frames = self.closed_frames.saturating_add(1);
            if self.closed_frames > 25 {
                self.noise_history[self.noise_count % 200] = rms.min(self.noise_high * 4.0) as f32;
                self.noise_count += 1;
                if self.noise_count.is_multiple_of(5) {
                    let filled = self.noise_count.min(200);
                    let mut history = self.noise_history;
                    history[..filled].sort_by(f32::total_cmp);
                    self.noise_high =
                        f64::from(history[((filled - 1) as f64 * 0.9).floor() as usize]);
                }
            }
        }
        if self.settings.mic_threshold_mode == ThresholdMode::Manual {
            if !in_transient && self.metrics.db >= self.settings.manual_threshold_value {
                if self.manual_hold == 0 {
                    self.mark_speech(self.frame_id - 14);
                }
                self.manual_hold = 38;
            } else if self.manual_hold > 0 {
                if !in_transient && self.metrics.db >= self.settings.manual_threshold_value - 8.0 {
                    self.manual_hold = 38;
                } else {
                    self.manual_hold -= 1;
                }
            }
        }
        let speaking = if self.settings.mic_threshold_mode == ThresholdMode::Manual {
            self.manual_hold > 0
        } else if analyzer_enabled {
            self.segment_open
        } else {
            true
        };
        self.gate_open = speaking;
        let write = self.write;
        self.frames[write] = SpeechFrame {
            samples: frame,
            id: self.frame_id,
            rms: rms as f32,
            speech: speaking,
            transient,
        };
        self.write = (write + 1) % 25;
        self.buffered += 1;
        let delay = if analyzer_enabled {
            24
        } else if noise_enabled {
            1
        } else {
            0
        };
        let read = (write + 25 - delay) % 25;
        let ready = self.buffered > delay as u64;
        let speech = ready && self.frames[read].speech;
        let delayed = self.frames[read].samples;
        let mut engine_input = delayed;
        let mut gain = 1.0;
        if ready && noise_enabled {
            let peak = delayed
                .iter()
                .fold(0.0_f64, |peak, sample| peak.max(f64::from(*sample).abs()));
            self.engine_peak = if peak > self.engine_peak {
                peak
            } else {
                self.engine_peak + 0.004 * (peak - self.engine_peak)
            };
            let slew = 10.0_f64.powf(0.25 / 20.0);
            gain = if self.engine_gain > self.engine_applied {
                self.engine_gain.min(self.engine_applied * slew)
            } else {
                self.engine_gain.max(self.engine_applied / slew)
            }
            .max(1.0);
            self.engine_applied = gain;
            if gain > 1.0 {
                for sample in &mut engine_input {
                    *sample = (f64::from(*sample) * gain).clamp(-0.999, 0.999) as f32;
                }
            }
            let rms = f64::from(self.frames[read].rms);
            if speech && rms >= (self.engine_rms * 0.35).max(0.0015) {
                self.engine_rms = if self.engine_frames == 0 {
                    rms
                } else {
                    self.engine_rms + 0.004 * (rms.min(self.engine_rms * 4.0) - self.engine_rms)
                };
                self.engine_frames = self.engine_frames.saturating_add(1);
                let target = (0.1 / self.engine_rms.max(0.0015))
                    .min(0.7 / self.engine_peak.max(0.0015))
                    .clamp(1.0, 31.62);
                self.engine_gain += (target - self.engine_gain)
                    * if self.engine_frames <= 50 {
                        0.05
                    } else {
                        0.008
                    };
            }
        }
        let mut processed = engine_input;
        let mut denoised = false;
        if ready && noise_enabled {
            if let Some(noise) = &mut self.noise {
                denoised = noise.process(&engine_input, &mut processed)
                    && processed.iter().all(|sample| sample.is_finite());
                if !denoised {
                    processed = engine_input;
                    self.metrics.model_failed = true;
                }
            }
        }
        if !ready {
            processed.fill(0.0);
            self.gate_gain = 0.0;
        } else if !noise_enabled && !analyzer_enabled {
            processed = delayed;
            self.gate_gain = 1.0;
            self.gate_hold = 0;
        } else {
            if denoised
                && self
                    .noise
                    .as_ref()
                    .is_some_and(|noise| noise.attenuation_limited())
            {
                let before: f64 = engine_input
                    .iter()
                    .map(|sample| f64::from(*sample).powi(2))
                    .sum();
                let after: f64 = processed
                    .iter()
                    .map(|sample| f64::from(*sample).powi(2))
                    .sum();
                let emptied = before > 1e-12
                    && 10.0 * (after.max(1e-12) / before).log10()
                        <= -f64::from((self.settings.suppression_strength - 3.0).max(9.0));
                if speech {
                    self.veto_frames = 0;
                    self.veto_latched = false;
                } else if emptied {
                    self.veto_frames = (self.veto_frames + 1).min(25);
                    if self.veto_frames >= 25 {
                        self.veto_latched = true;
                    }
                } else {
                    self.veto_frames = self.veto_frames.saturating_sub(2);
                    if self.veto_frames == 0 {
                        self.veto_latched = false;
                    }
                }
            } else {
                self.veto_frames = 0;
                self.veto_latched = false;
            }
            if speech {
                self.gate_hold = 32;
            } else if self.veto_latched {
                self.gate_hold = 0;
            } else {
                self.gate_hold = self.gate_hold.saturating_sub(1);
            }
            let target = if speech || self.gate_hold > 0 {
                1.0
            } else {
                0.0
            };
            for sample in &mut processed {
                self.gate_gain += (target - self.gate_gain)
                    * if target > self.gate_gain {
                        1.0 / 48.0
                    } else {
                        1.0 / 5760.0
                    };
                *sample = (f64::from(*sample) / gain * self.gate_gain) as f32;
            }
            if target == 0.0 && self.gate_gain <= 0.004 {
                self.gate_gain = 0.0;
            }
        }
        if noise_enabled {
            self.apply_level(&mut processed);
        }
        self.metrics.speaking =
            ready && !self.veto_latched && self.gate_gain > 0.05 && (speech || self.gate_hold > 0);
        *output = self.output_delay[self.output_index];
        self.output_delay[self.output_index] = processed;
        self.output_index = (self.output_index + 1) % 2;
        let mut decimated = [0.0_f64; 160];
        if self.decimator.process_block(&frame, &mut decimated).is_ok() {
            for sample in decimated {
                let sample = sample as f32;
                self.vad[self.vad_fill] = sample;
                self.vad_fill += 1;
                self.voicing.push_block(std::slice::from_ref(&sample));
                if self.vad_fill == 512 {
                    let voicing = self.voicing.measure();
                    let lag = voicing.lag;
                    let voiced = voicing.score >= 0.45
                        && lag > 0
                        && self.previous_lag > 0
                        && lag.abs_diff(self.previous_lag) as f64 / self.previous_lag as f64
                            <= 0.25;
                    self.previous_lag = lag;
                    self.voiced_hold = if voiced {
                        6
                    } else {
                        self.voiced_hold.saturating_sub(1)
                    };
                    if let Some(analyzer) = &mut self.analyzer {
                        match analyzer.probability(&self.vad, self.vad_sequence) {
                            Some(probability) if probability.is_finite() => {
                                self.probability(probability.clamp(0.0, 1.0), voiced, self.frame_id)
                            }
                            _ => {
                                self.segment_open = false;
                                self.metrics.model_failed = true;
                            }
                        }
                        self.vad_sequence = self.vad_sequence.saturating_add(1);
                    }
                    self.vad_fill = 0;
                }
            }
        }
        self.frame_id = self.frame_id.saturating_add(1);
        self.metrics
    }

    fn apply_level(&mut self, frame: &mut [f32; 480]) {
        self.shaper.set_speech_reference(if self.alc_frames > 0 {
            self.alc_rms.max(0.0015)
        } else {
            0.0
        });
        let mut peak = 0.0_f64;
        let mut squares = 0.0;
        for sample in frame.iter_mut() {
            let value = self.shaper.process(f64::from(*sample));
            *sample = value as f32;
            peak = peak.max(value.abs());
            squares += value * value;
        }
        self.alc_peak = if peak > self.alc_peak {
            peak
        } else {
            self.alc_peak + 0.004 * (peak - self.alc_peak)
        };
        let downstream = f64::from(self.settings.input_gain()).max(1.0);
        let gain = if peak > 0.0 {
            self.alc_gain.min((0.891 / (peak * downstream)).max(0.1))
        } else {
            self.alc_gain
        };
        for sample in frame {
            self.alc_applied += (gain - self.alc_applied)
                * if gain < self.alc_applied {
                    1.0 / 24.0
                } else {
                    1.0 / 5760.0
                };
            *sample = (f64::from(*sample) * self.alc_applied).clamp(-0.999, 0.999) as f32;
        }
        let rms = (squares / 480.0).sqrt();
        if self.gate_gain > 0.5 && peak >= 0.0015 {
            self.alc_rms = if self.alc_frames == 0 {
                rms
            } else {
                self.alc_rms + 0.004 * (rms.min(self.alc_rms * 4.0) - self.alc_rms)
            };
            self.alc_frames = self.alc_frames.saturating_add(1);
            let target = (0.065 / self.alc_rms.max(0.0015))
                .min(0.891 / (self.alc_peak.max(0.0015) * downstream))
                .clamp(1.0, 5.0);
            self.alc_gain +=
                (target - self.alc_gain) * if self.alc_frames <= 50 { 0.05 } else { 0.008 };
        }
    }
}
