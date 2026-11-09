use crate::audio_runtime::{AudioCleanup, AudioCleanupError};
use crate::media::{MediaError, MediaWorkerState};
use crate::screen::{ScreenSource, SourceKind};
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::audio_source::{native::NativeAudioSource, AudioSourceOptions};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use zabor_stream_audio::{CaptureError, Packet, Source, Target};

const FRAME_SAMPLES: usize = 960;
const QUEUE_SAMPLES: usize = FRAME_SAMPLES * 10;

pub struct StreamAudioCapture {
    pub(crate) source: NativeAudioSource,
    stop: CancellationToken,
    failed: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

struct PcmQueue(VecDeque<i16>);

impl PcmQueue {
    fn push(&mut self, packet: Packet) -> Result<(), CaptureError> {
        if !packet.samples.len().is_multiple_of(2)
            || packet.samples.len() > zabor_stream_audio::MAX_PACKET_FRAMES * 2
        {
            return Err(CaptureError::Unavailable);
        }
        if packet.discontinuity {
            self.0.clear();
        }
        let start = packet.samples.len().saturating_sub(QUEUE_SAMPLES);
        let extra = self
            .0
            .len()
            .saturating_add(packet.samples.len() - start)
            .saturating_sub(QUEUE_SAMPLES);
        self.0.drain(..extra);
        self.0.extend(packet.samples[start..].iter().map(|sample| {
            if sample.is_finite() {
                (sample.clamp(-1.0, 1.0) * 32768.0) as i16
            } else {
                0
            }
        }));
        Ok(())
    }

    fn frame(&mut self) -> Option<[i16; FRAME_SAMPLES]> {
        if self.0.len() < FRAME_SAMPLES {
            return None;
        }
        let mut frame = [0; FRAME_SAMPLES];
        for sample in &mut frame {
            *sample = self.0.pop_front().unwrap();
        }
        Some(frame)
    }
}

fn forward(
    stop: &CancellationToken,
    source: &NativeAudioSource,
    ready: oneshot::Sender<NativeAudioSource>,
    capture: impl FnOnce(
        &dyn Fn() -> bool,
        &mut dyn FnMut(Target) -> bool,
        &mut dyn FnMut(Packet) -> bool,
    ) -> Result<(), CaptureError>,
) -> Result<(), CaptureError> {
    let runtime = tokio::runtime::Handle::current();
    let mut ready = Some(ready);
    let mut queue = PcmQueue(VecDeque::with_capacity(QUEUE_SAMPLES));
    let mut delivery_failed = false;
    let result = capture(
        &|| stop.is_cancelled(),
        &mut |_| !stop.is_cancelled() && ready.take().unwrap().send(source.clone()).is_ok(),
        &mut |packet| {
            if stop.is_cancelled() {
                return false;
            }
            if queue.push(packet).is_err() {
                delivery_failed = true;
                return false;
            }
            while let Some(data) = queue.frame() {
                if stop.is_cancelled() {
                    return false;
                }
                let frame = AudioFrame {
                    data: Cow::Borrowed(&data),
                    sample_rate: 48000,
                    num_channels: 2,
                    samples_per_channel: 480,
                };
                if runtime.block_on(source.capture_frame(&frame)).is_err() {
                    delivery_failed = true;
                    return false;
                }
            }
            true
        },
    );
    result?;
    if delivery_failed {
        Err(CaptureError::Unavailable)
    } else {
        Ok(())
    }
}

impl StreamAudioCapture {
    pub(crate) async fn start(
        source: &ScreenSource,
        cleanup: Arc<AudioCleanup>,
    ) -> Result<Self, MediaError> {
        let source = match source.kind {
            SourceKind::Screen => Source::Screen,
            SourceKind::Window => Source::Window(source.id),
        };
        Self::start_with(
            cleanup,
            CancellationToken::new(),
            move |stop, source_audio, ready| {
                forward(stop, source_audio, ready, |cancelled, ready, deliver| {
                    zabor_stream_audio::run(source, cancelled, ready, deliver)
                })
            },
        )
        .await
    }

    async fn start_with(
        cleanup: Arc<AudioCleanup>,
        stop: CancellationToken,
        acquire: impl FnOnce(
                &CancellationToken,
                &NativeAudioSource,
                oneshot::Sender<NativeAudioSource>,
            ) -> Result<(), CaptureError>
            + Send
            + 'static,
    ) -> Result<Self, MediaError> {
        let prepared = cleanup.prepare_task().map_err(|_| MediaError::Backend)?;
        let guard = stop.clone().drop_guard();
        let failed = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let worker_failed = failed.clone();
        let worker_finished = finished.clone();
        let worker_stop = stop.clone();
        let (ready, received) = oneshot::channel();
        prepared.spawn_blocking_result(move || {
            let _state = MediaWorkerState::new(worker_failed.clone(), worker_finished);
            if worker_stop.is_cancelled() {
                return Ok(());
            }
            let source = NativeAudioSource::new(
                AudioSourceOptions {
                    echo_cancellation: false,
                    noise_suppression: false,
                    auto_gain_control: false,
                },
                48000,
                2,
                0,
            );
            let result = acquire(&worker_stop, &source, ready);
            drop(source);
            if result.is_err() {
                worker_failed.store(true, Ordering::Release);
            }
            if result == Err(CaptureError::Cleanup) {
                Err(AudioCleanupError::Incomplete)
            } else {
                Ok(())
            }
        });
        let source = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(MediaError::Backend),
            result = tokio::time::timeout(std::time::Duration::from_secs(5), received) => result.map_err(|_| MediaError::Backend)?.map_err(|_| MediaError::Backend)?,
        };
        if stop.is_cancelled() || failed.load(Ordering::Acquire) || finished.load(Ordering::Acquire)
        {
            return Err(MediaError::Backend);
        }
        guard.disarm();
        Ok(Self {
            source,
            stop,
            failed,
            finished,
        })
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    pub(crate) fn is_live(&self) -> bool {
        !self.stop.is_cancelled() && !self.failed() && !self.finished.load(Ordering::Acquire)
    }

    #[cfg(all(test, feature = "stream-audio-test-source"))]
    pub(crate) async fn synthetic(
        cleanup: Arc<AudioCleanup>,
        mode: u8,
        delay_ms: u32,
    ) -> Result<Self, MediaError> {
        Self::start_with(
            cleanup,
            CancellationToken::new(),
            move |stop, source, ready| {
                forward(stop, source, ready, |cancelled, ready, deliver| {
                    zabor_stream_audio::run_synthetic(
                        Source::Screen,
                        mode,
                        delay_ms,
                        cancelled,
                        ready,
                        deliver,
                    )
                })
            },
        )
        .await
    }
}

impl Drop for StreamAudioCapture {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[cfg(test)]
mod tests;
