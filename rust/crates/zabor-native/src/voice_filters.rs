use std::f64::consts::TAU;

const RATE: f64 = 48000.0;
const FFT: usize = 8192;
const BINS: usize = FFT / 2;
const BIN_HZ: f64 = RATE / FFT as f64;
const HARMONICS: [f64; 10] = [
    50.0, 60.0, 100.0, 120.0, 150.0, 180.0, 200.0, 240.0, 250.0, 300.0,
];

#[derive(Clone)]
pub struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b: [1.0, 0.0, 0.0],
            a: [0.0; 2],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }
}

impl Biquad {
    fn normalize(&mut self, b: [f64; 3], a: [f64; 3]) {
        self.b = b.map(|value| value / a[0]);
        self.a = [a[1] / a[0], a[2] / a[0]];
    }

    pub fn peaking(frequency: f64, quality: f64, gain: f64) -> Self {
        let amplitude = 10.0_f64.powf(gain / 40.0);
        let omega = TAU * frequency / RATE;
        let alpha = omega.sin() / (2.0 * quality);
        let cosine = omega.cos();
        let mut filter = Self::default();
        filter.normalize(
            [
                1.0 + alpha * amplitude,
                -2.0 * cosine,
                1.0 - alpha * amplitude,
            ],
            [
                1.0 + alpha / amplitude,
                -2.0 * cosine,
                1.0 - alpha / amplitude,
            ],
        );
        filter
    }

    pub fn high_shelf(frequency: f64, quality: f64, gain: f64) -> Self {
        let amplitude = 10.0_f64.powf(gain / 40.0);
        let omega = TAU * frequency / RATE;
        let alpha = omega.sin() / (2.0 * quality);
        let cosine = omega.cos();
        let root = 2.0 * amplitude.sqrt() * alpha;
        let mut filter = Self::default();
        filter.normalize(
            [
                amplitude * (amplitude + 1.0 + (amplitude - 1.0) * cosine + root),
                -2.0 * amplitude * (amplitude - 1.0 + (amplitude + 1.0) * cosine),
                amplitude * (amplitude + 1.0 + (amplitude - 1.0) * cosine - root),
            ],
            [
                amplitude + 1.0 - (amplitude - 1.0) * cosine + root,
                2.0 * (amplitude - 1.0 - (amplitude + 1.0) * cosine),
                amplitude + 1.0 - (amplitude - 1.0) * cosine - root,
            ],
        );
        filter
    }

    pub fn band_pass(frequency: f64, quality: f64) -> Self {
        let omega = TAU * frequency / RATE;
        let alpha = omega.sin() / (2.0 * quality);
        let mut filter = Self::default();
        filter.normalize(
            [alpha, 0.0, -alpha],
            [1.0 + alpha, -2.0 * omega.cos(), 1.0 - alpha],
        );
        filter
    }

    pub fn high_pass(frequency: f64, quality: f64) -> Self {
        let omega = TAU * frequency / RATE;
        let alpha = omega.sin() / (2.0 * quality);
        let cosine = omega.cos();
        let mut filter = Self::default();
        filter.normalize(
            [(1.0 + cosine) / 2.0, -(1.0 + cosine), (1.0 + cosine) / 2.0],
            [1.0 + alpha, -2.0 * cosine, 1.0 - alpha],
        );
        filter
    }

    fn tune_notch(&mut self, frequency: f64, quality: f64) {
        let omega = TAU * frequency / RATE;
        let alpha = omega.sin() / (2.0 * quality);
        self.normalize(
            [1.0, -2.0 * omega.cos(), 1.0],
            [1.0 + alpha, -2.0 * omega.cos(), 1.0 - alpha],
        );
    }

    pub fn process(&mut self, sample: f64) -> f64 {
        let mut value = self.b[0] * sample + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        if value.abs() < 1e-25 {
            value = 0.0;
        }
        self.x = [sample, self.x[0]];
        self.y = [value, self.y[0]];
        value
    }
}

pub struct VoiceShaper {
    mud: Biquad,
    presence: Biquad,
    air: Biquad,
    detector: Biquad,
    bell: Biquad,
    power: f64,
    threshold: f64,
}

impl Default for VoiceShaper {
    fn default() -> Self {
        Self {
            mud: Biquad::peaking(300.0, 0.9, -1.8),
            presence: Biquad::peaking(3600.0, 0.85, 2.2),
            air: Biquad::high_shelf(9000.0, 0.707, 1.2),
            detector: Biquad::band_pass(6300.0, 1.0),
            bell: Biquad::peaking(6800.0, 1.1, -5.0),
            power: 0.0,
            threshold: 0.0,
        }
    }
}

impl VoiceShaper {
    pub fn set_speech_reference(&mut self, rms: f64) {
        self.threshold = if rms > 0.0 { (rms * 0.26).powi(2) } else { 0.0 };
    }
    pub fn process(&mut self, sample: f64) -> f64 {
        let sample = self.mud.process(sample);
        let sample = self.presence.process(sample);
        let shaped = self.air.process(sample);
        let sibilant = self.detector.process(shaped);
        let power = sibilant * sibilant;
        self.power += (power - self.power)
            * if power > self.power {
                1.0 / 48.0
            } else {
                1.0 / 3840.0
            };
        let wet = self.bell.process(shaped);
        if self.threshold <= 0.0 || self.power <= self.threshold {
            return shaped;
        }
        let mix = (((self.power / self.threshold).sqrt() - 1.0) / 2.0).min(1.0);
        shaped + (wet - shaped) * mix
    }
}

struct ToneAnalyzer {
    samples: Vec<f32>,
    real: Vec<f64>,
    imag: Vec<f64>,
    power: Vec<f64>,
    prefix: Vec<f64>,
    window: Vec<f32>,
    cos: Vec<f32>,
    sin: Vec<f32>,
    reverse: Vec<usize>,
    scratch: Vec<f64>,
    fill: usize,
    stage: usize,
    countdown: usize,
    transforming: bool,
}

impl ToneAnalyzer {
    fn new() -> Self {
        Self {
            samples: vec![0.0; FFT],
            real: vec![0.0; FFT],
            imag: vec![0.0; FFT],
            power: vec![0.0; BINS],
            prefix: vec![0.0; BINS + 1],
            window: (0..FFT)
                .map(|i| (0.5 - 0.5 * (TAU * i as f64 / FFT as f64).cos()) as f32)
                .collect(),
            cos: (0..BINS)
                .map(|i| (TAU * i as f64 / FFT as f64).cos() as f32)
                .collect(),
            sin: (0..BINS)
                .map(|i| (TAU * i as f64 / FFT as f64).sin() as f32)
                .collect(),
            reverse: (0..FFT)
                .map(|i| i.reverse_bits() >> (usize::BITS - 13))
                .collect(),
            scratch: Vec::with_capacity(104),
            fill: 0,
            stage: 0,
            countdown: 0,
            transforming: false,
        }
    }

    fn push(&mut self, sample: f64, collecting: bool) -> bool {
        if self.transforming {
            self.countdown -= 1;
            if self.countdown > 0 {
                return false;
            }
            self.countdown = 64;
            self.run_stage();
            if self.stage < 13 {
                return false;
            }
            self.transforming = false;
            for bin in 0..BINS {
                self.power[bin] = self.real[bin] * self.real[bin] + self.imag[bin] * self.imag[bin];
            }
            return true;
        }
        if !collecting {
            self.fill = 0;
            return false;
        }
        self.samples[self.fill] = sample as f32;
        self.fill += 1;
        if self.fill < FFT {
            return false;
        }
        self.fill = 0;
        for index in 0..FFT {
            let source = self.reverse[index];
            self.real[index] = f64::from(self.samples[source]) * f64::from(self.window[source]);
            self.imag[index] = 0.0;
        }
        self.stage = 0;
        self.countdown = 64;
        self.transforming = true;
        false
    }

    fn run_stage(&mut self) {
        let size = 2 << self.stage;
        let half = size / 2;
        let step = FFT / size;
        for base in (0..FFT).step_by(size) {
            for offset in 0..half {
                let lower = base + offset;
                let upper = lower + half;
                let twiddle = offset * step;
                let cosine = f64::from(self.cos[twiddle]);
                let sine = f64::from(self.sin[twiddle]);
                let re = self.real[upper] * cosine + self.imag[upper] * sine;
                let im = self.imag[upper] * cosine - self.real[upper] * sine;
                self.real[upper] = self.real[lower] - re;
                self.imag[upper] = self.imag[lower] - im;
                self.real[lower] += re;
                self.imag[lower] += im;
            }
        }
        self.stage += 1;
    }

    fn interpolate(&self, peak: usize) -> f64 {
        if !(1..BINS - 1).contains(&peak) {
            return peak as f64 * BIN_HZ;
        }
        let left = self.power[peak - 1].max(1e-24).ln();
        let center = self.power[peak].max(1e-24).ln();
        let right = self.power[peak + 1].max(1e-24).ln();
        let curvature = left - 2.0 * center + right;
        if curvature >= 0.0 {
            return peak as f64 * BIN_HZ;
        }
        (peak as f64 + (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)) * BIN_HZ
    }

    fn prominence(&mut self, peak: usize, inner: usize, outer: usize) -> f64 {
        self.scratch.clear();
        for offset in inner..=outer {
            if self.scratch.len() > 101 {
                break;
            }
            if peak > offset {
                self.scratch.push(self.power[peak - offset]);
            }
            if peak + offset < BINS {
                self.scratch.push(self.power[peak + offset]);
            }
        }
        if self.scratch.is_empty() {
            return 0.0;
        }
        self.scratch.sort_by(f64::total_cmp);
        10.0 * (self.power[peak].max(1e-24) / self.scratch[self.scratch.len() / 2].max(1e-24))
            .log10()
    }

    fn harmonic(&mut self, nominal: f64) -> (f64, f64) {
        let center = (nominal / BIN_HZ).round() as usize;
        let mut peak = center.max(1);
        for bin in center.saturating_sub(1)..=center + 1 {
            if (1..BINS - 1).contains(&bin) && self.power[bin] > self.power[peak] {
                peak = bin;
            }
        }
        let hz = self.interpolate(peak);
        if (hz - nominal).abs() > 3.0 {
            return (0.0, nominal);
        }
        (self.prominence(peak, 3, 12), hz)
    }

    fn whine(&mut self, claimed: &[f64]) -> (f64, f64) {
        self.prefix[0] = 0.0;
        for bin in 0..BINS {
            self.prefix[bin + 1] = self.prefix[bin] + self.power[bin];
        }
        let mut peak = None;
        let mut best = -1.0;
        for bin in (400.0 / BIN_HZ).round() as usize..=(7000.0 / BIN_HZ).round() as usize {
            let hz = bin as f64 * BIN_HZ;
            if claimed
                .iter()
                .any(|taken| *taken > 0.0 && (hz - taken).abs() < 150.0)
            {
                continue;
            }
            let low = bin.saturating_sub(60);
            let high = (bin + 61).min(BINS);
            let average = (self.prefix[high] - self.prefix[low]) / (high - low) as f64;
            let ratio = self.power[bin] / average.max(1e-24);
            if ratio > best {
                best = ratio;
                peak = Some(bin);
            }
        }
        match peak {
            Some(peak) => (self.prominence(peak, 10, 60), self.interpolate(peak)),
            None => (0.0, 0.0),
        }
    }
}

struct NotchSlot {
    filter: Biquad,
    bandwidth: f64,
    retune: f64,
    engage: f64,
    release: f64,
    frequency: f64,
    pending: f64,
    mix: f64,
    engaged: bool,
    confirm: u32,
    release_streak: u32,
}

impl NotchSlot {
    fn new(bandwidth: f64, retune: f64, engage: f64, release: f64, hz: f64) -> Self {
        let mut slot = Self {
            filter: Biquad::default(),
            bandwidth,
            retune,
            engage,
            release,
            frequency: 0.0,
            pending: 0.0,
            mix: 0.0,
            engaged: false,
            confirm: 0,
            release_streak: 0,
        };
        if hz > 0.0 {
            slot.tune(hz);
        }
        slot
    }
    fn tune(&mut self, hz: f64) {
        self.frequency = hz;
        self.filter.tune_notch(hz, (hz / self.bandwidth).max(0.5));
    }
    fn observe(&mut self, prominence: f64, frequency: f64) {
        if prominence >= self.engage {
            if self.frequency > 0.0 && (frequency - self.frequency).abs() > self.retune {
                self.pending = frequency;
                self.engaged = false;
                self.confirm = 0;
                self.release_streak = 0;
                return;
            }
            self.release_streak = 0;
            self.confirm += 1;
            if self.confirm < 6 {
                return;
            }
            self.confirm = 6;
            self.engaged = true;
            if self.mix <= 0.0 || (frequency - self.frequency).abs() <= 1.0 {
                self.tune(frequency);
            }
            return;
        }
        self.confirm = 0;
        if prominence > self.release {
            return;
        }
        self.release_streak += 1;
        if self.release_streak >= 6 {
            self.release_streak = 6;
            self.engaged = false;
        }
    }
    fn process(&mut self, sample: f64) -> f64 {
        let target = if self.engaged { 1.0 } else { 0.0 };
        if self.mix < target {
            self.mix = (self.mix + 1.0 / 2400.0).min(target);
        } else if self.mix > target {
            self.mix = (self.mix - 1.0 / 2400.0).max(target);
            if self.mix <= 0.0 && self.pending > 0.0 {
                self.tune(self.pending);
                self.pending = 0.0;
            }
        }
        let filtered = self.filter.process(sample);
        if self.mix <= 0.0 {
            sample
        } else {
            sample + (filtered - sample) * self.mix
        }
    }
}

pub struct TonalSuppressor {
    analyzer: ToneAnalyzer,
    hum: [NotchSlot; 10],
    whine: [NotchSlot; 3],
}

impl Default for TonalSuppressor {
    fn default() -> Self {
        Self {
            analyzer: ToneAnalyzer::new(),
            hum: HARMONICS.map(|hz| NotchSlot::new(2.0, 8.0, 8.0, 5.0, hz)),
            whine: std::array::from_fn(|_| NotchSlot::new(60.0, 12.0, 10.0, 6.0, 0.0)),
        }
    }
}

impl TonalSuppressor {
    pub fn process(&mut self, sample: f64, adapting: bool) -> f64 {
        if self.analyzer.push(sample, adapting) {
            for (slot, nominal) in self.hum.iter_mut().zip(HARMONICS) {
                let (prominence, hz) = self.analyzer.harmonic(nominal);
                slot.observe(prominence, hz);
            }
            let mut claimed = [0.0; 3];
            for (index, slot) in self.whine.iter_mut().enumerate() {
                let (prominence, hz) = self.analyzer.whine(&claimed[..index]);
                slot.observe(prominence, hz);
                claimed[index] = hz;
            }
        }
        self.hum
            .iter_mut()
            .chain(self.whine.iter_mut())
            .fold(sample, |value, slot| slot.process(value))
    }
}
