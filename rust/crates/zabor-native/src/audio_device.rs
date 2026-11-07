use crate::audio_route::{DeviceInventory, DeviceRecovery};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
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
        let device = enumerator.get_device(id).map_err(|_| DeviceError)?;
        if device.get_state().ok() != Some(DeviceState::Active)
            || device.get_direction() != *direction
        {
            return Err(DeviceError);
        }
        Ok(device)
    }
}

fn inventory(direction: &Direction) -> Result<DeviceInventory, DeviceError> {
    let enumerator = DeviceEnumerator::new().map_err(|_| DeviceError)?;
    let mut endpoints = Vec::new();
    for device in &enumerator
        .get_device_collection(direction)
        .map_err(|_| DeviceError)?
    {
        let Ok(device) = device else { continue };
        if device.get_state().ok() == Some(DeviceState::Active) {
            if let Ok(id) = device.get_id() {
                endpoints.push(id);
            }
        }
    }
    let default = enumerator
        .get_default_device(direction)
        .ok()
        .and_then(|device| device.get_id().ok());
    Ok(DeviceInventory { endpoints, default })
}

type InventoryReader = Box<dyn FnMut(&Direction) -> Result<DeviceInventory, DeviceError> + Send>;

struct ManagedRoute {
    select: Box<dyn Fn() -> Result<String, DeviceError> + Send>,
    restarting: Box<dyn FnMut() + Send>,
    direction: Direction,
    checked_at: Instant,
    active: Option<String>,
    requested: String,
    inventory: InventoryReader,
}

impl ManagedRoute {
    fn new<S, R>(select: S, restarting: R, direction: Direction) -> Self
    where
        S: Fn() -> Result<String, DeviceError> + Send + 'static,
        R: FnMut() + Send + 'static,
    {
        Self {
            select: Box::new(select),
            restarting: Box::new(restarting),
            direction,
            checked_at: Instant::now(),
            active: None,
            requested: String::new(),
            inventory: Box::new(inventory),
        }
    }

    fn target(&mut self) -> Result<String, DeviceError> {
        (self.inventory)(&self.direction)?
            .resolve(&self.requested)
            .ok_or(DeviceError)
    }

    fn changed(&mut self) -> bool {
        if (self.select)()
            .map(|requested| requested != self.requested)
            .unwrap_or(true)
        {
            return true;
        }
        if self.checked_at.elapsed() < Duration::from_millis(500) {
            return false;
        }
        self.checked_at = Instant::now();
        match (self.inventory)(&self.direction) {
            Ok(devices) => devices.resolve(&self.requested) != self.active,
            Err(_) => false,
        }
    }
}

fn run_sessions<F>(
    id: &str,
    stop: &CancellationToken,
    ready: &mpsc::SyncSender<Result<(), DeviceError>>,
    mut route: Option<ManagedRoute>,
    mut run: F,
) -> Result<(), DeviceError>
where
    F: FnMut(&str, &mut dyn FnMut() -> bool, &dyn Fn()) -> Result<(), DeviceError>,
{
    let notified = std::cell::Cell::new(false);
    let mut recovery = DeviceRecovery::default();
    loop {
        if stop.is_cancelled() {
            return Ok(());
        }
        if recovery.expired(Instant::now()) {
            return Err(DeviceError);
        }
        if !recovery.can_retry(Instant::now()) {
            std::thread::sleep(Duration::from_millis(25));
            continue;
        }
        let target = match &mut route {
            Some(route) => (route.select)().and_then(|requested| {
                route.requested = requested;
                route.target()
            }),
            None => Ok(id.to_owned()),
        };
        if stop.is_cancelled() {
            return Ok(());
        }
        let started_at = std::cell::Cell::new(None);
        let result = match target {
            Ok(target) => {
                if let Some(route) = &mut route {
                    (route.restarting)();
                    route.active = Some(target.clone());
                    route.checked_at = Instant::now();
                }
                run(
                    &target,
                    &mut || {
                        stop.is_cancelled() || route.as_mut().is_some_and(ManagedRoute::changed)
                    },
                    &|| {
                        started_at.set(Some(Instant::now()));
                        if !notified.replace(true) {
                            let _ = ready.send(Ok(()));
                        }
                    },
                )
            }
            Err(error) => Err(error),
        };
        if stop.is_cancelled() {
            return Ok(());
        }
        let Some(route) = &mut route else {
            return result;
        };
        (route.restarting)();
        if started_at
            .get()
            .is_some_and(|start| start.elapsed() >= Duration::from_millis(500))
        {
            recovery.recovered();
        }
        recovery.lost(Instant::now());
        if result.is_err() {
            recovery.failed(Instant::now());
        }
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

fn drain_capture<C>(
    queue: &mut VecDeque<u8>,
    samples: &mut [f32; 960],
    channels: usize,
    gap: &mut bool,
    process: &mut C,
    changed: &mut dyn FnMut() -> bool,
) -> bool
where
    C: FnMut(&[f32], usize, bool),
{
    let bytes = 480 * channels * 4;
    while queue.len() >= bytes {
        if changed() {
            return true;
        }
        let data = queue.make_contiguous();
        for (sample, data) in samples[..480 * channels]
            .iter_mut()
            .zip(data[..bytes].as_chunks::<4>().0)
        {
            let value = f32::from_le_bytes(*data);
            *sample = if value.is_finite() { value } else { 0.0 };
        }
        queue.drain(..bytes);
        process(&samples[..480 * channels], channels, *gap);
        *gap = false;
    }
    false
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
        Self::capture_routed(id, loopback, make_processor, CancellationToken::new(), None)
    }

    pub fn capture_managed<S, R, F, C>(
        select: S,
        restarting: R,
        make_processor: F,
        stop: CancellationToken,
    ) -> Result<Self, DeviceError>
    where
        S: Fn() -> Result<String, DeviceError> + Send + 'static,
        R: FnMut() + Send + 'static,
        F: FnOnce(CancellationToken) -> Result<C, DeviceError> + Send + 'static,
        C: FnMut(&[f32], usize, bool) + 'static,
    {
        Self::capture_routed(
            String::new(),
            false,
            make_processor,
            stop,
            Some(ManagedRoute::new(select, restarting, Direction::Capture)),
        )
    }

    fn capture_routed<F, C>(
        id: String,
        loopback: bool,
        make_processor: F,
        stop: CancellationToken,
        route: Option<ManagedRoute>,
    ) -> Result<Self, DeviceError>
    where
        F: FnOnce(CancellationToken) -> Result<C, DeviceError> + Send + 'static,
        C: FnMut(&[f32], usize, bool) + 'static,
    {
        let watch_stall = route.is_some();
        Self::spawn("zabor-capture", stop, move |stop, ready| {
            let _com = Com::initialize()?;
            if stop.is_cancelled() {
                return Ok(());
            }
            let mut process = make_processor(stop.clone())?;
            if stop.is_cancelled() {
                return Err(DeviceError);
            }
            run_sessions(&id, &stop, &ready, route, |id, changed, started| {
                if changed() {
                    return Ok(());
                }
                let device = select(
                    id,
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
                if changed() {
                    return Ok(());
                }
                client.start_stream().map_err(|_| DeviceError)?;
                started();
                let result = (|| {
                    let mut gap = true;
                    let mut last_event = Instant::now();
                    while !stop.is_cancelled() {
                        if changed() {
                            return Ok(());
                        }
                        match event.wait_for_event(50) {
                            Ok(()) => {
                                last_event = Instant::now();
                            }
                            Err(WasapiError::EventTimeout)
                                if !watch_stall
                                    || last_event.elapsed() < Duration::from_secs(2) =>
                            {
                                continue
                            }
                            Err(_) => return Err(DeviceError),
                        }
                        while capture
                            .get_next_packet_size()
                            .map_err(|_| DeviceError)?
                            .is_some_and(|frames| frames > 0)
                        {
                            if changed() {
                                return Ok(());
                            }
                            let old = queue.len();
                            let info = capture
                                .read_from_device_to_deque(&mut queue)
                                .map_err(|_| DeviceError)?;
                            if info.flags.data_discontinuity {
                                queue.drain(..old);
                                gap = true;
                            }
                            if queue.len() > bytes * 4 {
                                queue.clear();
                                gap = true;
                            }
                            if drain_capture(
                                &mut queue,
                                &mut samples,
                                channels,
                                &mut gap,
                                &mut process,
                                changed,
                            ) {
                                return Ok(());
                            }
                        }
                    }
                    Ok(())
                })();
                let _ = client.stop_stream();
                result
            })
        })
    }

    pub fn render<F>(id: String, mut render: F) -> Result<Self, DeviceError>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        Self::render_with_feedback(id, move |samples| render(samples), |_, _| {})
    }

    pub fn render_with_feedback<F, P>(id: String, render: F, played: P) -> Result<Self, DeviceError>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
        P: FnMut(&[f32], u32) + Send + 'static,
    {
        Self::render_routed(id, render, played, CancellationToken::new(), None)
    }

    pub fn render_managed<S, R, F, P>(
        select: S,
        restarting: R,
        render: F,
        played: P,
        stop: CancellationToken,
    ) -> Result<Self, DeviceError>
    where
        S: Fn() -> Result<String, DeviceError> + Send + 'static,
        R: FnMut() + Send + 'static,
        F: FnMut(&mut [f32]) + Send + 'static,
        P: FnMut(&[f32], u32) + Send + 'static,
    {
        Self::render_routed(
            String::new(),
            render,
            played,
            stop,
            Some(ManagedRoute::new(select, restarting, Direction::Render)),
        )
    }

    fn render_routed<F, P>(
        id: String,
        mut render: F,
        mut played: P,
        stop: CancellationToken,
        route: Option<ManagedRoute>,
    ) -> Result<Self, DeviceError>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
        P: FnMut(&[f32], u32) + Send + 'static,
    {
        let watch_stall = route.is_some();
        Self::spawn("zabor-playback", stop, move |stop, ready| {
            let _com = Com::initialize()?;
            run_sessions(&id, &stop, &ready, route, |id, changed, started| {
                if changed() {
                    return Ok(());
                }
                let device = select(id, &Direction::Render)?;
                let client = initialize(&device, &Direction::Render, 2)?;
                let event = client.set_get_eventhandle().map_err(|_| DeviceError)?;
                let output = client.get_audiorenderclient().map_err(|_| DeviceError)?;
                let capacity = client.get_buffer_size().map_err(|_| DeviceError)? as usize;
                if capacity > 48000 {
                    return Err(DeviceError);
                }
                let mut samples = vec![0.0_f32; capacity * 2];
                let mut bytes = vec![0_u8; capacity * 8];
                if changed() {
                    return Ok(());
                }
                output
                    .write_to_device(capacity, &bytes, None)
                    .map_err(|_| DeviceError)?;
                client.start_stream().map_err(|_| DeviceError)?;
                started();
                let result = (|| {
                    let mut last_event = Instant::now();
                    while !stop.is_cancelled() {
                        if changed() {
                            return Ok(());
                        }
                        match event.wait_for_event(50) {
                            Ok(()) => {
                                last_event = Instant::now();
                            }
                            Err(WasapiError::EventTimeout)
                                if !watch_stall
                                    || last_event.elapsed() < Duration::from_secs(2) =>
                            {
                                continue
                            }
                            Err(_) => return Err(DeviceError),
                        }
                        let count = client
                            .get_available_space_in_frames()
                            .map_err(|_| DeviceError)? as usize;
                        if count > capacity {
                            return Err(DeviceError);
                        }
                        if count == 0 {
                            continue;
                        }
                        samples[..count * 2].fill(0.0);
                        render(&mut samples[..count * 2]);
                        if changed() {
                            return Ok(());
                        }
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
                        played(&samples[..count * 2], capacity.div_ceil(48) as u32);
                    }
                    Ok(())
                })();
                let _ = client.stop_stream();
                result
            })
        })
    }

    fn spawn<F>(name: &str, stop: CancellationToken, run: F) -> Result<Self, DeviceError>
    where
        F: FnOnce(
                CancellationToken,
                mpsc::SyncSender<Result<(), DeviceError>>,
            ) -> Result<(), DeviceError>
            + Send
            + 'static,
    {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn fake_route(selected: Arc<Mutex<String>>) -> ManagedRoute {
        let mut route = ManagedRoute::new(
            move || Ok(selected.lock().unwrap().clone()),
            || {},
            Direction::Capture,
        );
        route.inventory = Box::new(|_| {
            Ok(DeviceInventory {
                endpoints: vec!["usb".into(), "builtin".into()],
                default: Some("builtin".into()),
            })
        });
        route
    }

    #[test]
    fn managed_route_changes_selection_immediately_without_waiting_for_inventory_refresh() {
        let selected = Arc::new(Mutex::new("usb".into()));
        let mut route = fake_route(selected.clone());
        route.requested = "usb".into();
        route.active = Some("usb".into());
        assert!(!route.changed());
        *selected.lock().unwrap() = "default".into();
        assert!(route.changed());
        route.requested = "default".into();
        assert_eq!(route.target().unwrap(), "builtin");
    }

    #[test]
    fn inventory_failure_keeps_the_existing_route_but_missing_devices_restart_it() {
        let mut route = fake_route(Arc::new(Mutex::new("usb".into())));
        route.requested = "usb".into();
        route.active = Some("usb".into());
        route.checked_at = Instant::now() - Duration::from_secs(1);
        route.inventory = Box::new(|_| Err(DeviceError));
        assert!(!route.changed());
        route.checked_at = Instant::now() - Duration::from_secs(1);
        route.inventory = Box::new(|_| Ok(DeviceInventory::default()));
        assert!(route.changed());
    }

    #[test]
    fn default_hotplug_and_preferred_device_return_are_detected_without_an_id_rewrite() {
        let mut route = fake_route(Arc::new(Mutex::new("usb".into())));
        route.requested = "usb".into();
        route.active = Some("usb".into());
        route.checked_at = Instant::now() - Duration::from_secs(1);
        route.inventory = Box::new(|_| {
            Ok(DeviceInventory {
                endpoints: vec!["builtin".into()],
                default: Some("builtin".into()),
            })
        });
        assert!(route.changed());
        assert_eq!(route.target().unwrap(), "builtin");
        route.active = Some("builtin".into());
        route.checked_at = Instant::now() - Duration::from_secs(1);
        route.inventory = Box::new(|_| {
            Ok(DeviceInventory {
                endpoints: vec!["usb".into(), "builtin".into()],
                default: Some("builtin".into()),
            })
        });
        assert!(route.changed());
        assert_eq!(route.requested, "usb");
        route.requested = "default".into();
        route.select = Box::new(|| Ok("default".into()));
        route.checked_at = Instant::now() - Duration::from_secs(1);
        route.inventory = Box::new(|_| {
            Ok(DeviceInventory {
                endpoints: vec!["usb".into()],
                default: Some("usb".into()),
            })
        });
        assert!(route.changed());
    }

    #[test]
    fn managed_sessions_reuse_non_send_owner_state_and_notify_ready_only_once() {
        let selected = Arc::new(Mutex::new("usb".into()));
        let observed = Arc::new(Mutex::new(Vec::new()));
        let output = observed.clone();
        let stop = CancellationToken::new();
        let mut stream = DeviceStream::spawn("zabor-route-fixture", stop, move |stop, ready| {
            let owner = std::rc::Rc::new(std::cell::Cell::new(0));
            let selection = selected.clone();
            let route = fake_route(selected);
            run_sessions("", &stop, &ready, Some(route), |id, changed, started| {
                owner.set(owner.get() + 1);
                output.lock().unwrap().push((
                    std::thread::current().id(),
                    owner.get(),
                    id.to_owned(),
                ));
                started();
                if owner.get() == 3 {
                    stop.cancel();
                } else {
                    *selection.lock().unwrap() = if id == "usb" { "default" } else { "usb" }.into();
                    assert!(changed());
                }
                Ok(())
            })
        })
        .unwrap();
        stream.thread.take().unwrap().join().unwrap();
        assert!(!stream.failed());
        let observed = observed.lock().unwrap();
        assert_eq!(observed.len(), 3);
        assert!(observed
            .iter()
            .all(|(thread, _, _)| *thread == observed[0].0));
        assert_eq!(
            observed
                .iter()
                .map(|(_, count, id)| (*count, id.as_str()))
                .collect::<Vec<_>>(),
            [(1, "usb"), (2, "builtin"), (3, "usb")]
        );
    }

    #[test]
    fn cancellation_before_session_start_does_not_inspect_or_open_devices() {
        let stop = CancellationToken::new();
        stop.cancel();
        let (ready, received) = mpsc::sync_channel(1);
        let mut route = fake_route(Arc::new(Mutex::new("usb".into())));
        route.inventory = Box::new(|_| panic!("cancelled session inspected devices"));
        run_sessions("", &stop, &ready, Some(route), |_, _, _| {
            panic!("cancelled session opened a device")
        })
        .unwrap();
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn cancellation_interrupts_missing_device_recovery_without_a_ready_notification() {
        let stop = CancellationToken::new();
        let cancel = stop.clone();
        let (ready, received) = mpsc::sync_channel(1);
        let mut route = fake_route(Arc::new(Mutex::new("usb".into())));
        route.inventory = Box::new(move |_| {
            cancel.cancel();
            Ok(DeviceInventory::default())
        });
        run_sessions("", &stop, &ready, Some(route), |_, _, _| {
            panic!("missing endpoint was opened")
        })
        .unwrap();
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn cancellation_after_inventory_resolution_prevents_device_initialization() {
        let stop = CancellationToken::new();
        let cancel = stop.clone();
        let (ready, received) = mpsc::sync_channel(1);
        let mut route = fake_route(Arc::new(Mutex::new("usb".into())));
        route.inventory = Box::new(move |_| {
            cancel.cancel();
            Ok(DeviceInventory {
                endpoints: vec!["usb".into()],
                default: Some("usb".into()),
            })
        });
        run_sessions("", &stop, &ready, Some(route), |_, _, _| {
            panic!("cancelled endpoint was opened")
        })
        .unwrap();
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn continuous_capture_backlog_stops_between_frames_after_a_device_switch() {
        let mut queue: VecDeque<u8> = (0..480 * 4).flat_map(|_| 0.1_f32.to_le_bytes()).collect();
        let frames = std::cell::Cell::new(0);
        let mut samples = [0.0; 960];
        let mut gap = true;
        let changed = drain_capture(
            &mut queue,
            &mut samples,
            1,
            &mut gap,
            &mut |samples: &[f32], channels, gap| {
                assert_eq!(samples, [0.1; 480]);
                assert_eq!(channels, 1);
                assert!(gap);
                frames.set(frames.get() + 1);
            },
            &mut || frames.get() >= 1,
        );
        assert!(changed);
        assert_eq!(frames.get(), 1);
        assert_eq!(queue.len(), 480 * 4 * 3);
        assert!(!gap);
    }

    #[test]
    fn capture_drain_keeps_stereo_frames_and_reports_a_gap_only_on_the_first_frame() {
        let mut queue: VecDeque<u8> = (0..480 * 2 * 2)
            .flat_map(|_| 0.2_f32.to_le_bytes())
            .collect();
        let mut samples = [0.0; 960];
        let mut gap = true;
        let mut observed = Vec::new();
        assert!(!drain_capture(
            &mut queue,
            &mut samples,
            2,
            &mut gap,
            &mut |samples: &[f32], channels, gap| {
                assert_eq!(samples, [0.2; 960]);
                assert_eq!(channels, 2);
                observed.push(gap);
            },
            &mut || false
        ));
        assert_eq!(observed, [true, false]);
        assert!(queue.is_empty());
    }
}
