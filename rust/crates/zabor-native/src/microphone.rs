use crate::audio_mix::{AudioControls, AudioMixer};
use crate::audio_settings::AudioSettings;
#[cfg(feature = "neural-dsp")]
use crate::audio_settings::ThresholdMode;
use crate::media::MediaError;
use crate::smart_gate::{MicMetrics, SmartGate};
#[cfg(feature = "neural-dsp")]
use crate::smart_gate::{NoiseProcessor, VoiceProbability};
use crate::voice_activity::VoiceActivity;
use crate::voice_filters::Biquad;
use futures_util::FutureExt;
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::audio_source::{native::NativeAudioSource, AudioSourceOptions};
use libwebrtc::native::apm::AudioProcessingModule;
use std::borrow::Cow;
use std::sync::{atomic::Ordering, Arc};
use tokio_util::sync::CancellationToken;

#[allow(clippy::approx_constant)]
const RUMBLE_QS: [f64; 3] = [0.5177, 0.7071, 1.9319];

pub fn audio_source() -> NativeAudioSource {
    NativeAudioSource::new(
        AudioSourceOptions {
            echo_cancellation: false,
            noise_suppression: false,
            auto_gain_control: false,
        },
        48000,
        1,
        0,
    )
}

fn smart(settings: &AudioSettings) -> Result<Option<SmartGate>, MediaError> {
    if !settings.noise_suppression || settings.ultra_low_latency {
        return Ok(None);
    }
    #[cfg(feature = "neural-dsp")]
    {
        let noise: Box<dyn NoiseProcessor> = Box::new(
            crate::neural::Denoiser::new(
                settings.smart_noise_model,
                settings.suppression_strength,
                settings.post_filter_beta(),
            )
            .map_err(|_| MediaError::Microphone)?,
        );
        let analyzer: Option<Box<dyn VoiceProbability>> =
            if settings.speech_analyzer && settings.mic_threshold_mode == ThresholdMode::Auto {
                Some(Box::new(
                    crate::neural::Silero::new().map_err(|_| MediaError::Microphone)?,
                ))
            } else {
                None
            };
        Ok(Some(SmartGate::new(
            settings.clone(),
            Some(noise),
            analyzer,
        )))
    }
    #[cfg(not(feature = "neural-dsp"))]
    Err(MediaError::Microphone)
}

pub struct MicProcessor {
    controls: Arc<AudioControls>,
    mixer: Arc<AudioMixer>,
    settings: AudioSettings,
    revision: u64,
    epoch: u64,
    smart: Option<SmartGate>,
    rumble: [Biquad; 3],
    activity: VoiceActivity,
    apm: Option<AudioProcessingModule>,
    failed: bool,
}

impl MicProcessor {
    pub fn new(controls: Arc<AudioControls>, mixer: Arc<AudioMixer>) -> Result<Self, MediaError> {
        let (settings, revision) = {
            let settings = controls
                .settings
                .lock()
                .map_err(|_| MediaError::Microphone)?;
            (
                settings.clone(),
                controls.settings_revision.load(Ordering::Acquire),
            )
        };
        let epoch = controls.capture_epoch.load(Ordering::Acquire);
        let smart = smart(&settings)?;
        let apm = settings
            .echo_cancellation
            .then(|| AudioProcessingModule::new(true, false, false, false));
        Ok(Self {
            controls,
            mixer,
            settings,
            revision,
            epoch,
            smart,
            rumble: RUMBLE_QS.map(|q| Biquad::high_pass(70.0, q)),
            activity: VoiceActivity::default(),
            apm,
            failed: false,
        })
    }

    fn reset(&mut self) -> Result<(), MediaError> {
        if self.smart.as_mut().is_some_and(|smart| !smart.reset()) {
            return Err(MediaError::Microphone);
        }
        self.rumble = RUMBLE_QS.map(|q| Biquad::high_pass(70.0, q));
        self.activity.reset();
        self.apm = self
            .settings
            .echo_cancellation
            .then(|| AudioProcessingModule::new(true, false, false, false));
        if let Ok(mut reverse) = self.mixer.reverse.lock() {
            reverse.clear();
        }
        self.failed = false;
        Ok(())
    }

    fn refresh(&mut self, gap: bool) -> Result<(), MediaError> {
        let revision = self.controls.settings_revision.load(Ordering::Acquire);
        let epoch = self.controls.capture_epoch.load(Ordering::Acquire);
        let mut reset = gap || epoch != self.epoch;
        self.epoch = epoch;
        if revision != self.revision {
            let (settings, revision) = {
                let settings = self
                    .controls
                    .settings
                    .lock()
                    .map_err(|_| MediaError::Microphone)?;
                (
                    settings.clone(),
                    self.controls.settings_revision.load(Ordering::Acquire),
                )
            };
            let rebuild = settings.noise_suppression != self.settings.noise_suppression
                || settings.ultra_low_latency != self.settings.ultra_low_latency
                || settings.smart_noise_model != self.settings.smart_noise_model
                || settings.speech_analyzer != self.settings.speech_analyzer
                || settings.mic_threshold_mode != self.settings.mic_threshold_mode;
            reset |= rebuild || settings.echo_cancellation != self.settings.echo_cancellation;
            self.settings = settings;
            self.revision = revision;
            if rebuild {
                self.smart = smart(&self.settings)?;
            }
            if let Some(smart) = &mut self.smart {
                smart.configure_level(
                    self.settings.input_volume,
                    self.settings.manual_threshold_value,
                    self.settings.suppression_strength,
                );
            }
        }
        if reset {
            self.reset()?;
        }
        Ok(())
    }

    pub fn process(&mut self, samples: &[f32], channels: usize, gap: bool) -> [i16; 480] {
        let mut pcm = [0; 480];
        let result = self.process_inner(samples, channels, gap, &mut pcm);
        if result.is_err() {
            self.failed = true;
            pcm.fill(0);
            self.controls.speaking.store(false, Ordering::Release);
            self.controls
                .mic_db
                .store((-100.0_f32).to_bits(), Ordering::Release);
        }
        self.controls
            .processor_failed
            .store(self.failed, Ordering::Release);
        pcm
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    fn process_inner(
        &mut self,
        samples: &[f32],
        channels: usize,
        gap: bool,
        pcm: &mut [i16; 480],
    ) -> Result<(), MediaError> {
        if !(1..=2).contains(&channels)
            || samples.len() != 480 * channels
            || samples.iter().any(|sample| !sample.is_finite())
        {
            return Err(MediaError::Microphone);
        }
        self.refresh(gap)?;
        if self.failed {
            return Err(MediaError::Microphone);
        }
        let muted = self.controls.effectively_muted();
        let mut input = [0.0_f32; 480];
        if !muted {
            for (frame, sample) in samples.chunks_exact(channels).zip(&mut input) {
                *sample = (frame.iter().map(|sample| f64::from(*sample)).sum::<f64>()
                    / channels as f64)
                    .clamp(-1.0, 1.0) as f32;
            }
        }
        if let Some(apm) = &mut self.apm {
            let mut reverse = [0_i16; 480];
            if let Ok(mut queued) = self.mixer.reverse.try_lock() {
                while queued.len() >= 480 {
                    for sample in &mut reverse {
                        *sample = queued.pop_front().unwrap_or(0);
                    }
                    apm.process_reverse_stream(&mut reverse, 48000, 1)
                        .map_err(|_| MediaError::Microphone)?;
                }
            }
            for (sample, target) in input.iter().zip(pcm.iter_mut()) {
                *target = to_pcm(*sample);
            }
            apm.set_stream_delay_ms(
                (self.controls.render_delay_ms.load(Ordering::Acquire) + 10).min(500) as i32,
            )
            .map_err(|_| MediaError::Microphone)?;
            apm.process_stream(pcm, 48000, 1)
                .map_err(|_| MediaError::Microphone)?;
            for (sample, source) in input.iter_mut().zip(pcm.iter()) {
                *sample = f32::from(*source) / 32768.0;
            }
        } else if let Ok(mut reverse) = self.mixer.reverse.try_lock() {
            reverse.clear();
        }
        let mut output = input;
        let metrics = if let Some(smart) = &mut self.smart {
            for sample in &mut input {
                let mut value = f64::from(*sample);
                for filter in &mut self.rumble {
                    value = filter.process(value);
                }
                *sample = value as f32;
            }
            smart.process(&input, &mut output, muted)
        } else {
            let rms = (input
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>()
                / 480.0)
                .sqrt();
            MicMetrics {
                db: (20.0 * rms.max(1e-6).log10()).clamp(-100.0, 0.0) as f32,
                speaking: !muted && self.activity.process(&input),
                ..MicMetrics::default()
            }
        };
        if metrics.model_failed || output.iter().any(|sample| !sample.is_finite()) {
            return Err(MediaError::Microphone);
        }
        for (target, sample) in pcm.iter_mut().zip(output) {
            *target = if self.settings.ultra_low_latency {
                to_pcm(sample)
            } else {
                to_pcm(peak_guard(sample * self.settings.input_gain()))
            };
        }
        let muted = self.controls.effectively_muted()
            || self.controls.capture_epoch.load(Ordering::Acquire) != self.epoch;
        if muted {
            pcm.fill(0);
        }
        self.controls.mic_db.store(
            if muted { -100.0_f32 } else { metrics.db }.to_bits(),
            Ordering::Release,
        );
        self.controls
            .speaking
            .store(!muted && metrics.speaking, Ordering::Release);
        Ok(())
    }
}

fn peak_guard(value: f32) -> f32 {
    let value = f64::from(value.clamp(-1.0, 1.0));
    let magnitude = value.abs();
    if magnitude <= 0.98 {
        value as f32
    } else {
        (value.signum()
            * (0.98
                + 0.015 * (1.0 - (-3.0 * (magnitude - 0.98) / 0.02).exp())
                    / (1.0 - (-3.0_f64).exp()))) as f32
    }
}

fn to_pcm(sample: f32) -> i16 {
    (f64::from(sample.clamp(-1.0, 1.0)) * if sample < 0.0 { 32768.0 } else { 32767.0 }).round()
        as i16
}

pub struct MicSubmission {
    source: NativeAudioSource,
    controls: Arc<AudioControls>,
    stop: CancellationToken,
}

impl MicSubmission {
    pub fn new(
        source: NativeAudioSource,
        controls: Arc<AudioControls>,
        stop: CancellationToken,
    ) -> Self {
        Self {
            source,
            controls,
            stop,
        }
    }
    pub fn submit(&self, pcm: &mut [i16; 480], epoch: u64) -> Result<(), MediaError> {
        if self.stop.is_cancelled() {
            pcm.fill(0);
            return Ok(());
        }
        if self.controls.effectively_muted()
            || self.controls.processor_failed.load(Ordering::Acquire)
            || self.controls.capture_epoch.load(Ordering::Acquire) != epoch
        {
            pcm.fill(0);
        }
        let frame = AudioFrame {
            data: Cow::Borrowed(pcm),
            sample_rate: 48000,
            num_channels: 1,
            samples_per_channel: 480,
        };
        match self.source.capture_frame(&frame).now_or_never() {
            Some(Ok(())) => Ok(()),
            _ => {
                self.controls
                    .processor_failed
                    .store(true, Ordering::Release);
                Err(MediaError::Microphone)
            }
        }
    }
}
