use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use wasapi::{
    AudioClient, Device, DeviceEnumerator, DeviceState, Direction, SampleType, StreamMode,
    WasapiError, WaveFormat,
};

#[derive(Clone)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub input: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("native audio device is unavailable")]
pub struct DeviceError;

struct Com;
impl Com {
    fn initialize() -> Result<Self, DeviceError> {
        wasapi::initialize_mta().ok().map_err(|_| DeviceError)?;
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        wasapi::deinitialize();
    }
}

pub fn devices() -> Result<Vec<AudioDevice>, DeviceError> {
    let _com = Com::initialize()?;
    let enumerator = DeviceEnumerator::new().map_err(|_| DeviceError)?;
    let mut devices = Vec::new();
    for (direction, input) in [(Direction::Capture, true), (Direction::Render, false)] {
        for device in &enumerator
            .get_device_collection(&direction)
            .map_err(|_| DeviceError)?
        {
            let device = device.map_err(|_| DeviceError)?;
            if device.get_state().ok() != Some(DeviceState::Active) {
                continue;
            }
            devices.push(AudioDevice {
                id: device.get_id().map_err(|_| DeviceError)?,
                name: device.get_friendlyname().map_err(|_| DeviceError)?,
                input,
            });
        }
    }
    Ok(devices)
}

fn select(id: &str, direction: &Direction) -> Result<Device, DeviceError> {
    let enumerator = DeviceEnumerator::new().map_err(|_| DeviceError)?;
    if id.is_empty() || id == "default" {
        enumerator
            .get_default_device(direction)
            .map_err(|_| DeviceError)
    } else {
        enumerator.get_device(id).map_err(|_| DeviceError)
    }
}

fn initialize(
    device: &Device,
    direction: &Direction,
    channels: usize,
) -> Result<AudioClient, DeviceError> {
    let mut client = device.get_iaudioclient().map_err(|_| DeviceError)?;
    let format = WaveFormat::new(32, 32, &SampleType::Float, 48000, channels, None);
    let (_, minimum) = client.get_device_period().map_err(|_| DeviceError)?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: minimum.max(100_000),
    };
    client
        .initialize_client(&format, direction, &mode)
        .map_err(|_| DeviceError)?;
    Ok(client)
}

pub struct DeviceStream {
    stop: CancellationToken,
    failed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl DeviceStream {
    pub fn capture<F, C>(id: String, loopback: bool, make_processor: F) -> Result<Self, DeviceError>
    where
        F: FnOnce(CancellationToken) -> Result<C, DeviceError> + Send + 'static,
        C: FnMut(&[f32], usize, bool) + 'static,
    {
        Self::spawn("zabor-capture", move |stop, ready| {
            let _com = Com::initialize()?;
            let mut process = make_processor(stop.clone())?;
            if stop.is_cancelled() {
                return Err(DeviceError);
            }
            let device = select(
                &id,
                &if loopback {
                    Direction::Render
                } else {
                    Direction::Capture
                },
            )?;
            let channels = if loopback {
                2
            } else {
                usize::from(
                    device
                        .get_iaudioclient()
                        .map_err(|_| DeviceError)?
                        .get_mixformat()
                        .map_err(|_| DeviceError)?
                        .get_nchannels(),
                )
                .clamp(1, 2)
            };
            let client = initialize(&device, &Direction::Capture, channels)?;
            let event = client.set_get_eventhandle().map_err(|_| DeviceError)?;
            let capture = client.get_audiocaptureclient().map_err(|_| DeviceError)?;
            let mut queue = VecDeque::with_capacity(480 * channels * 4 * 4);
            let mut samples = [0.0_f32; 960];
            let bytes = 480 * channels * 4;
            client.start_stream().map_err(|_| DeviceError)?;
            let _ = ready.send(Ok(()));
            let result = (|| {
                let mut gap = false;
                while !stop.is_cancelled() {
                    match event.wait_for_event(50) {
                        Ok(()) => {}
                        Err(WasapiError::EventTimeout) => continue,
                        Err(_) => return Err(DeviceError),
                    }
                    while capture
                        .get_next_packet_size()
                        .map_err(|_| DeviceError)?
                        .is_some_and(|frames| frames > 0)
                    {
                        let old = queue.len();
                        let info = capture
                            .read_from_device_to_deque(&mut queue)
                            .map_err(|_| DeviceError)?;
                        if info.flags.data_discontinuity {
                            queue.drain(..old);
                            gap = true;
                        }
                        if queue.len() > bytes * 20 {
                            queue.clear();
                            gap = true;
                        }
                        while queue.len() >= bytes {
                            let data = queue.make_contiguous();
                            for (sample, data) in samples[..480 * channels]
                                .iter_mut()
                                .zip(data[..bytes].as_chunks::<4>().0)
                            {
                                let value = f32::from_le_bytes(*data);
                                *sample = if value.is_finite() { value } else { 0.0 };
                            }
                            queue.drain(..bytes);
                            if stop.is_cancelled() {
                                break;
                            }
                            process(&samples[..480 * channels], channels, gap);
                            gap = false;
                        }
                    }
                }
                Ok(())
            })();
            let _ = client.stop_stream();
            result
        })
    }

    pub fn render<F>(id: String, mut render: F) -> Result<Self, DeviceError>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        Self::spawn("zabor-playback", move |stop, ready| {
            let _com = Com::initialize()?;
            let device = select(&id, &Direction::Render)?;
            let client = initialize(&device, &Direction::Render, 2)?;
            let event = client.set_get_eventhandle().map_err(|_| DeviceError)?;
            let output = client.get_audiorenderclient().map_err(|_| DeviceError)?;
            let capacity = client.get_buffer_size().map_err(|_| DeviceError)? as usize;
            if capacity > 48000 {
                return Err(DeviceError);
            }
            let mut samples = vec![0.0_f32; capacity * 2];
            let mut bytes = vec![0_u8; capacity * 8];
            output
                .write_to_device(capacity, &bytes, None)
                .map_err(|_| DeviceError)?;
            client.start_stream().map_err(|_| DeviceError)?;
            let _ = ready.send(Ok(()));
            let result = (|| {
                while !stop.is_cancelled() {
                    match event.wait_for_event(50) {
                        Ok(()) => {}
                        Err(WasapiError::EventTimeout) => continue,
                        Err(_) => return Err(DeviceError),
                    }
                    let count = client
                        .get_available_space_in_frames()
                        .map_err(|_| DeviceError)? as usize;
                    if count > capacity {
                        return Err(DeviceError);
                    }
                    samples[..count * 2].fill(0.0);
                    render(&mut samples[..count * 2]);
                    for (sample, bytes) in samples[..count * 2]
                        .iter()
                        .zip(bytes[..count * 8].as_chunks_mut::<4>().0)
                    {
                        let sample = if sample.is_finite() {
                            sample.clamp(-1.0, 1.0)
                        } else {
                            0.0
                        };
                        *bytes = sample.to_le_bytes();
                    }
                    output
                        .write_to_device(count, &bytes[..count * 8], None)
                        .map_err(|_| DeviceError)?;
                }
                Ok(())
            })();
            let _ = client.stop_stream();
            result
        })
    }

    fn spawn<F>(name: &str, run: F) -> Result<Self, DeviceError>
    where
        F: FnOnce(
                CancellationToken,
                mpsc::SyncSender<Result<(), DeviceError>>,
            ) -> Result<(), DeviceError>
            + Send
            + 'static,
    {
        let stop = CancellationToken::new();
        let worker_stop = stop.clone();
        let failed = Arc::new(AtomicBool::new(false));
        let worker_failed = failed.clone();
        let (ready, status) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name(name.into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                if run(worker_stop, ready).is_err() {
                    worker_failed.store(true, Ordering::Release);
                }
            })
            .map_err(|_| DeviceError)?;
        let stream = Self {
            stop,
            failed,
            thread: Some(thread),
        };
        match status.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(())) => Ok(stream),
            _ => Err(DeviceError),
        }
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
            || (self
                .thread
                .as_ref()
                .is_some_and(|thread| thread.is_finished())
                && !self.stop.is_cancelled())
    }
    pub fn cancel(&self) {
        self.stop.cancel();
    }
}

impl Drop for DeviceStream {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
