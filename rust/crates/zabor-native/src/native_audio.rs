use crate::audio_device::{DeviceError, DeviceStream};
use crate::audio_mix::{AudioControls, AudioMixer};
use crate::media::MediaError;
use crate::microphone::{audio_source, MicProcessor, MicSubmission};
use libwebrtc::audio_source::native::NativeAudioSource;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio_util::sync::CancellationToken;

pub struct NativeAudio {
    pub source: NativeAudioSource,
    pub mixer: Arc<AudioMixer>,
    enabled: Arc<AtomicBool>,
    stop: CancellationToken,
    capture: DeviceStream,
    render: DeviceStream,
}

impl NativeAudio {
    pub async fn start(
        controls: Arc<AudioControls>,
        stop: CancellationToken,
    ) -> Result<Self, MediaError> {
        let guard = stop.clone().drop_guard();
        let worker_stop = stop.clone();
        let audio = tokio::task::spawn_blocking(move || {
            if worker_stop.is_cancelled() {
                return Err(MediaError::Microphone);
            }
            let settings = controls
                .settings
                .lock()
                .map_err(|_| MediaError::Microphone)?
                .clone();
            controls.processor_failed.store(false, Ordering::Release);
            let source = audio_source();
            let mixer = Arc::new(AudioMixer::new(controls.clone()));
            let enabled = Arc::new(AtomicBool::new(false));
            let active = enabled.clone();
            let render_mix = mixer.clone();
            let played_mix = mixer.clone();
            let render_controls = controls.clone();
            let feedback_controls = controls.clone();
            let render_stop = worker_stop.clone();
            let mut render_revision = controls.settings_revision.load(Ordering::Acquire);
            let render = DeviceStream::render_with_feedback(
                settings.selected_output,
                move |samples| {
                    let revision = render_controls.settings_revision.load(Ordering::Acquire);
                    if revision != render_revision {
                        if let Ok(settings) = render_controls.settings.try_lock() {
                            render_mix.update_gains(&settings);
                            render_revision = revision;
                        }
                    }
                    render_mix.render_pcm(samples);
                    if !active.load(Ordering::Acquire)
                        || render_stop.is_cancelled()
                        || render_controls.effectively_deafened()
                    {
                        samples.fill(0.0);
                    }
                },
                move |samples, delay| {
                    played_mix.played(samples);
                    feedback_controls
                        .render_delay_ms
                        .store(delay, Ordering::Release);
                },
            )
            .map_err(|_| MediaError::Microphone)?;
            let microphone_controls = controls.clone();
            let microphone_mix = mixer.clone();
            let microphone_source = source.clone();
            let active = enabled.clone();
            let capture_stop = worker_stop.clone();
            let capture =
                DeviceStream::capture(settings.selected_input, false, move |device_stop| {
                    let mut processor =
                        MicProcessor::new(microphone_controls.clone(), microphone_mix)
                            .map_err(|_| DeviceError)?;
                    if capture_stop.is_cancelled() {
                        device_stop.cancel();
                        return Err(DeviceError);
                    }
                    let submit = MicSubmission::new(
                        microphone_source,
                        microphone_controls,
                        capture_stop.clone(),
                    );
                    Ok(move |samples: &[f32], channels: usize, gap: bool| {
                        if capture_stop.is_cancelled() {
                            device_stop.cancel();
                            return;
                        }
                        if !active.load(Ordering::Acquire) {
                            return;
                        }
                        let mut pcm = processor.process(samples, channels, gap);
                        if submit.submit(&mut pcm, processor.epoch()).is_err() {
                            device_stop.cancel();
                        }
                    })
                })
                .map_err(|_| MediaError::Microphone)?;
            if worker_stop.is_cancelled() {
                return Err(MediaError::Microphone);
            }
            Ok(Self {
                source,
                mixer,
                enabled,
                stop: worker_stop,
                capture,
                render,
            })
        })
        .await
        .map_err(|_| MediaError::Microphone)??;
        guard.disarm();
        Ok(audio)
    }
    pub fn activate(&self) {
        self.enabled.store(true, Ordering::Release);
    }
    pub fn failed(&self) -> bool {
        self.capture.failed() || self.render.failed()
    }
    pub fn cancel(&self) {
        self.enabled.store(false, Ordering::Release);
        self.stop.cancel();
        self.capture.cancel();
        self.render.cancel();
        self.mixer.clear();
    }
}

impl Drop for NativeAudio {
    fn drop(&mut self) {
        self.cancel();
    }
}
