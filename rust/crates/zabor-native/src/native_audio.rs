use crate::audio_device::{DeviceError, DeviceStream};
use crate::audio_mix::{AudioControls, AudioMixer};
use crate::audio_runtime::{
    AudioCleanupError, AudioDemand, AudioFlow, CaptureInput, ProcessorRetry, SharedAudio,
    SharedAudioDevice,
};
use crate::media::MediaError;
use crate::mic_test::MicTest;
use crate::microphone::{audio_source, MicProcessor, MicSubmission};
use libwebrtc::audio_source::native::NativeAudioSource;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Instant;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub struct NativeAudio {
    pub source: NativeAudioSource,
    pub mixer: Arc<AudioMixer>,
    demand: Arc<AudioDemand>,
    legacy_generation: Option<u64>,
    stop: CancellationToken,
    capture: DeviceStream,
    render: DeviceStream,
    controls: Arc<AudioControls>,
    processor_retry: Arc<ProcessorRetry>,
}

impl NativeAudio {
    pub async fn start(
        controls: Arc<AudioControls>,
        stop: CancellationToken,
    ) -> Result<Self, MediaError> {
        let demand = Arc::new(AudioDemand::new(controls.clone()));
        let generation = demand.reserve().ok_or(MediaError::Microphone)?;
        let mut audio =
            Self::start_shared(controls, demand, Arc::new(MicTest::default()), stop).await?;
        audio.legacy_generation = Some(generation);
        Ok(audio)
    }

    pub async fn start_shared(
        controls: Arc<AudioControls>,
        demand: Arc<AudioDemand>,
        mic_test: Arc<MicTest>,
        stop: CancellationToken,
    ) -> Result<Self, MediaError> {
        let guard = stop.clone().drop_guard();
        let worker_stop = stop.clone();
        let audio = tokio::task::spawn_blocking(move || {
            if worker_stop.is_cancelled() {
                return Err(MediaError::Microphone);
            }
            controls.processor_failed.store(false, Ordering::Release);
            let source = audio_source();
            let mixer = Arc::new(AudioMixer::new(controls.clone()));
            let flow = Arc::new(AudioFlow {
                controls: controls.clone(),
                demand: demand.clone(),
                mic_test,
                mixer: mixer.clone(),
                output_session: AtomicU64::new(0),
                capture_voice: AtomicU64::new(0),
                stop: worker_stop.clone(),
            });
            let render_mix = mixer.clone();
            let render_controls = controls.clone();
            let render_flow = flow.clone();
            let played_flow = flow.clone();
            let output_flow = flow.clone();
            let output_selection = controls.clone();
            let output_restarting = controls.clone();
            let output_mix = mixer.clone();
            let mut render_revision = controls.settings_revision.load(Ordering::Acquire);
            let mut global_gain = (controls
                .settings
                .lock()
                .map_err(|_| MediaError::Microphone)?
                .output_volume
                / 100.0)
                .clamp(0.0, 2.0);
            let mut remote = vec![0.0; 96000];
            let mut local = vec![0.0; 96000];
            let pending = Arc::new(Mutex::new(None));
            let render_pending = pending.clone();
            let render = DeviceStream::render_managed(
                move || {
                    output_selection
                        .settings
                        .lock()
                        .map(|settings| settings.selected_output.clone())
                        .map_err(|_| DeviceError)
                },
                move || {
                    output_flow.output_session.fetch_add(1, Ordering::AcqRel);
                    output_mix.restart_output();
                    output_restarting.output_route_epoch.store(
                        output_restarting
                            .output_device_epoch
                            .load(Ordering::Acquire),
                        Ordering::Release,
                    );
                },
                move |samples| {
                    let revision = render_controls.settings_revision.load(Ordering::Acquire);
                    if revision != render_revision {
                        if let Ok(settings) = render_controls.settings.try_lock() {
                            render_mix.update_gains(&settings);
                            global_gain = (settings.output_volume / 100.0).clamp(0.0, 2.0);
                            render_revision = revision;
                        }
                    }
                    if samples.len() > remote.len() {
                        samples.fill(0.0);
                        return;
                    }
                    let route = render_flow.output_session.load(Ordering::Acquire);
                    let ticket = render_flow.render(
                        &mut remote[..samples.len()],
                        &mut local[..samples.len()],
                        samples,
                        global_gain,
                    );
                    if let Ok(mut pending) = render_pending.lock() {
                        *pending = Some((route, ticket));
                    } else {
                        samples.fill(0.0);
                    }
                },
                move |samples, delay| {
                    if let Ok(mut pending) = pending.lock() {
                        if let Some((route, ticket)) = pending.take() {
                            played_flow.played(samples, delay, route, ticket);
                        }
                    }
                },
                worker_stop.child_token(),
            )
            .map_err(|_| MediaError::Microphone)?;
            let microphone_controls = controls.clone();
            let processor_retry = Arc::new(ProcessorRetry::new(worker_stop.clone()));
            let capture_retry = processor_retry.clone();
            let microphone_mix = mixer.clone();
            let microphone_source = source.clone();
            let capture_flow = flow;
            let capture_stop = worker_stop.clone();
            let input_selection = controls.clone();
            let input_restarting = controls.clone();
            let capture = DeviceStream::capture_managed(
                move || {
                    input_selection
                        .settings
                        .lock()
                        .map(|settings| settings.selected_input.clone())
                        .map_err(|_| DeviceError)
                },
                move || {
                    input_restarting.capture_restarting();
                    input_restarting.input_route_epoch.store(
                        input_restarting.input_device_epoch.load(Ordering::Acquire),
                        Ordering::Release,
                    );
                },
                move |device_stop| {
                    let mut processor =
                        MicProcessor::new(microphone_controls.clone(), microphone_mix)
                            .map_err(|_| DeviceError)?;
                    if capture_stop.is_cancelled() {
                        device_stop.cancel();
                        return Err(DeviceError);
                    }
                    let submit = MicSubmission::new(
                        microphone_source,
                        microphone_controls.clone(),
                        device_stop.clone(),
                    );
                    Ok(move |samples: &[f32], channels: usize, gap: bool| {
                        if capture_stop.is_cancelled() {
                            device_stop.cancel();
                            return;
                        }
                        let recovery = capture_retry.take();
                        if let Some(ready) = &recovery {
                            if processor.rebuild().is_err() || capture_stop.is_cancelled() {
                                ready.send_replace(Some(false));
                                return;
                            }
                        }
                        if capture_flow
                            .capture(
                                &mut processor,
                                &submit,
                                CaptureInput {
                                    samples,
                                    channels,
                                    gap,
                                },
                                Instant::now(),
                            )
                            .is_err()
                        {
                            device_stop.cancel();
                        }
                        if let Some(ready) = recovery {
                            ready.send_replace(Some(
                                !capture_stop.is_cancelled()
                                    && !device_stop.is_cancelled()
                                    && !microphone_controls
                                        .processor_failed
                                        .load(Ordering::Acquire),
                            ));
                        }
                    })
                },
                worker_stop.child_token(),
            )
            .map_err(|_| MediaError::Microphone)?;
            if worker_stop.is_cancelled() {
                return Err(MediaError::Microphone);
            }
            Ok(Self {
                source,
                mixer,
                demand,
                legacy_generation: None,
                stop: worker_stop,
                capture,
                render,
                controls,
                processor_retry,
            })
        })
        .await
        .map_err(|_| MediaError::Microphone)??;
        guard.disarm();
        Ok(audio)
    }
    pub fn activate(&self) {
        if let Some(generation) = self.legacy_generation.filter(|_| !self.stop.is_cancelled()) {
            self.demand.activate(generation);
        }
    }
    pub fn failed(&self) -> bool {
        self.runtime_failed() || self.controls.processor_failed.load(Ordering::Acquire)
    }
    fn runtime_failed(&self) -> bool {
        self.stop.is_cancelled()
            || self.capture.failed()
            || self.capture.is_cancelled()
            || self.render.failed()
            || self.render.is_cancelled()
    }
    pub fn cancel(&self) {
        if let Some(generation) = self.legacy_generation {
            self.demand.revoke(generation);
        }
        self.stop.cancel();
        self.processor_retry.cancel();
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

impl SharedAudioDevice for NativeAudio {
    fn failed(&self) -> bool {
        NativeAudio::failed(self)
    }

    fn cancel(&self) {
        NativeAudio::cancel(self);
    }

    fn runtime_failed(&self) -> bool {
        NativeAudio::runtime_failed(self)
    }

    fn retry_processor(&self) -> Option<watch::Receiver<Option<bool>>> {
        if self.runtime_failed() {
            None
        } else {
            self.processor_retry.request()
        }
    }

    fn processor_recovering(&self) -> bool {
        self.processor_retry.recovering()
    }

    fn finish_shutdown(&self) -> Result<(), AudioCleanupError> {
        let capture = self.capture.join_shutdown();
        let render = self.render.join_shutdown();
        capture
            .and(render)
            .map_err(|_| AudioCleanupError::Incomplete)
    }
}

impl SharedAudio<NativeAudio> {
    pub fn native(controls: Arc<AudioControls>, stop: CancellationToken) -> Arc<Self> {
        Self::new(controls.clone(), stop, move |demand, mic_test, stop| {
            Box::pin(NativeAudio::start_shared(
                controls.clone(),
                demand,
                mic_test,
                stop,
            ))
        })
    }
}

#[cfg(feature = "audio-test-source")]
pub mod test_source;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_mix::AudioMixer;
    use crate::audio_settings::AudioSettings;
    use std::sync::mpsc;
    use std::time::Duration;

    #[tokio::test]
    async fn tracked_native_shutdown_joins_both_owner_threads_even_after_capture_panic_with_live_arcs(
    ) {
        for capture_panics in [false, true] {
            let controls = Arc::new(AudioControls::new(AudioSettings::default()));
            let stop = CancellationToken::new();
            let (capture_release, capture_blocked) = mpsc::channel();
            let (capture_done, capture_finished) = mpsc::channel();
            let capture = DeviceStream::test_worker(stop.child_token(), move |_, ready| {
                ready.send(Ok(())).unwrap();
                capture_blocked
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
                capture_done.send(()).unwrap();
                if capture_panics {
                    panic!("Expected native capture fixture panic");
                }
                Ok(())
            })
            .unwrap();
            let (render_release, render_blocked) = mpsc::channel();
            let (render_done, render_finished) = mpsc::channel();
            let render = DeviceStream::test_worker(stop.child_token(), move |_, ready| {
                ready.send(Ok(())).unwrap();
                render_blocked.recv_timeout(Duration::from_secs(3)).unwrap();
                render_done.send(()).unwrap();
                Ok(())
            })
            .unwrap();
            let audio = NativeAudio {
                source: audio_source(),
                mixer: Arc::new(AudioMixer::new(controls.clone())),
                demand: Arc::new(AudioDemand::new(controls.clone())),
                legacy_generation: None,
                processor_retry: Arc::new(ProcessorRetry::new(stop.clone())),
                stop,
                capture,
                render,
                controls: controls.clone(),
            };
            let prepared = Arc::new(Mutex::new(Some(audio)));
            let shared = SharedAudio::new(controls, CancellationToken::new(), move |_, _, _| {
                let audio = prepared.lock().unwrap().take().unwrap();
                Box::pin(async move { Ok(audio) })
            });
            let monitor = shared.monitor(true);
            let device = monitor.ready().await.unwrap();
            let retained_source = device.source.clone();
            let retained_mixer = device.mixer.clone();
            shared.reset();
            assert!(device.capture.is_cancelled());
            assert!(device.render.is_cancelled());
            capture_release.send(()).unwrap();
            capture_finished
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(30), shared.wait_for_cleanup())
                    .await
                    .is_err()
            );
            assert!(render_finished.try_recv().is_err());
            render_release.send(()).unwrap();
            let result = tokio::time::timeout(Duration::from_secs(1), shared.wait_for_cleanup())
                .await
                .unwrap();
            assert_eq!(result.is_err(), capture_panics);
            render_finished
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            assert_eq!(device.capture.join_shutdown().is_err(), capture_panics);
            assert!(device.render.join_shutdown().is_ok());
            assert_eq!(shared.wait_for_cleanup().await.is_err(), capture_panics);
            assert!(Arc::ptr_eq(&retained_mixer, &device.mixer));
            drop(retained_source);
            drop(retained_mixer);
            drop(device);
            drop(monitor);
            println!("native shutdown capture_panics={capture_panics} live_device_source_mixer=true both_owner_threads_joined=true held_render_waited=true failure_sticky=true source_destructor_bounds=false physical_audio=false");
        }
    }
}
