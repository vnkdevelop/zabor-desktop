mod protocol;
pub use protocol::{key_safe, Packet, Signal, CHANNEL_ID, CHANNEL_LABEL};
pub mod desktop;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn valid(self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= 65536
            && self.height <= 65536
            && self.left.checked_add(self.width).is_some()
            && self.top.checked_add(self.height).is_some()
    }

    pub fn point(self, x: f32, y: f32) -> Option<(i32, i32)> {
        if !self.valid()
            || !x.is_finite()
            || !y.is_finite()
            || !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
        {
            return None;
        }
        Some((
            self.left + (f64::from(x) * f64::from(self.width - 1)).round_ties_even() as i32,
            self.top + (f64::from(y) * f64::from(self.height - 1)).round_ties_even() as i32,
        ))
    }

    pub fn absolute(self, point: (i32, i32)) -> Option<(i32, i32)> {
        if !self.valid() || self.width < 2 || self.height < 2 {
            return None;
        }
        let x = i64::from(point.0) - i64::from(self.left);
        let y = i64::from(point.1) - i64::from(self.top);
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        Some((
            (x * 65535 / i64::from(self.width - 1)) as i32,
            (y * 65535 / i64::from(self.height - 1)) as i32,
        ))
    }

    fn contains(self, point: (i32, i32)) -> bool {
        self.valid()
            && point.0 >= self.left
            && point.1 >= self.top
            && i64::from(point.0) < i64::from(self.left) + i64::from(self.width)
            && i64::from(point.1) < i64::from(self.top) + i64::from(self.height)
    }
}

pub struct CaptureLease {
    window: Option<u64>,
    live: AtomicBool,
    frame: Mutex<Option<(Rect, Instant)>>,
}

impl CaptureLease {
    pub fn new(window: Option<u64>) -> Arc<Self> {
        Arc::new(Self {
            window,
            live: AtomicBool::new(true),
            frame: Mutex::new(None),
        })
    }

    pub fn update(&self, rect: Rect) {
        if !rect.valid() {
            self.revoke();
            return;
        }
        if let Ok(mut frame) = self.frame.lock() {
            if self.live.load(Ordering::Acquire) {
                *frame = Some((rect, Instant::now()));
            }
        } else {
            self.revoke();
        }
    }

    pub fn snapshot(&self) -> Option<Rect> {
        if !self.live.load(Ordering::Acquire) {
            return None;
        }
        self.frame
            .lock()
            .ok()?
            .as_ref()
            .filter(|(_, at)| at.elapsed() <= Duration::from_millis(750))
            .map(|(rect, _)| *rect)
    }

    pub fn window(&self) -> Option<u64> {
        self.window
    }

    pub fn revoke(&self) {
        self.live.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Ready,
    HostActive,
    HostIdle,
    Revoked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("native remote input is unavailable")]
    Unavailable,
    #[error("native remote input cleanup is incomplete")]
    Cleanup,
}

trait Backend {
    fn activity(&mut self) -> Result<(bool, bool), InputError>;
    fn foreground(&self, lease: &CaptureLease) -> bool;
    fn cursor(&self) -> Result<(i32, i32), InputError>;
    fn move_to(&mut self, point: (i32, i32)) -> Result<(), InputError>;
    fn mouse(&mut self, button: u8, down: bool) -> Result<(), InputError>;
    fn wheel(&mut self, vertical: i16, horizontal: i16) -> Result<(), InputError>;
    fn key(&mut self, vk: u16, down: bool) -> Result<(), InputError>;
    fn physical_modifiers(&self) -> u8;
    fn host_cursor(&mut self, visible: bool) -> Result<(), InputError>;
    fn poison(&mut self) {}
    fn finish(&mut self) -> Result<(), InputError>;
}

struct Owner<B: Backend> {
    backend: B,
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u8>,
    cleanup_failed: Arc<AtomicBool>,
    finished: bool,
}

impl<B: Backend> Owner<B> {
    fn modifiers(&self) -> u8 {
        self.backend.physical_modifiers()
            | u8::from(self.keys.contains(&0x10))
            | (u8::from(self.keys.contains(&0x11)) << 1)
            | (u8::from(self.keys.contains(&0x12)) << 2)
    }

    fn input(
        &mut self,
        lease: &CaptureLease,
        rect: Rect,
        packet: Packet,
        host_active: bool,
    ) -> Result<(), InputError> {
        if host_active && !packet.is_release() {
            return Ok(());
        }
        let foreground = self.backend.foreground(lease);
        match packet {
            Packet::Move { x, y } => {
                if foreground {
                    if let Some(point) = rect.point(x, y) {
                        self.backend.move_to(point)?;
                    }
                }
            }
            Packet::Mouse { button, down } => {
                if button > 2 {
                    return Ok(());
                }
                if down {
                    if foreground
                        && rect.contains(self.backend.cursor()?)
                        && !self.buttons.contains(&button)
                    {
                        self.buttons.insert(button);
                        self.backend.mouse(button, true)?;
                    }
                } else if self.buttons.contains(&button) {
                    self.backend.mouse(button, false)?;
                    self.buttons.remove(&button);
                }
            }
            Packet::Wheel {
                vertical,
                horizontal,
            } => {
                if foreground && rect.contains(self.backend.cursor()?) {
                    self.backend.wheel(vertical, horizontal)?;
                }
            }
            Packet::Key {
                vk,
                down,
                modifiers,
            } => {
                if down {
                    if foreground && key_safe(vk, modifiers | self.modifiers()) {
                        self.keys.insert(vk);
                        self.backend.key(vk, true)?;
                    }
                } else if self.keys.contains(&vk) {
                    self.backend.key(vk, false)?;
                    self.keys.remove(&vk);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn release(&mut self) -> Result<(), InputError> {
        let mut failed = false;
        self.keys.retain(|vk| {
            let error = self.backend.key(*vk, false).is_err();
            failed |= error;
            error
        });
        self.buttons.retain(|button| {
            let error = self.backend.mouse(*button, false).is_err();
            failed |= error;
            error
        });
        if failed {
            self.cleanup_failed.store(true, Ordering::Release);
            Err(InputError::Cleanup)
        } else {
            Ok(())
        }
    }

    fn finish(&mut self) -> Result<(), InputError> {
        let released = self.release();
        if released.is_err() {
            self.backend.poison();
        }
        let result = self.backend.finish();
        if released.is_err() || result.is_err() {
            self.backend.poison();
            self.cleanup_failed.store(true, Ordering::Release);
            return Err(InputError::Cleanup);
        }
        self.finished = true;
        Ok(())
    }
}

impl<B: Backend> Drop for Owner<B> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}

fn drive<B: Backend>(
    backend: B,
    lease: Arc<CaptureLease>,
    packets: mpsc::Receiver<Packet>,
    cancelled: impl Fn() -> bool,
    mut notify: impl FnMut(Event) -> bool,
    cleanup_failed: Arc<AtomicBool>,
) -> Result<(), InputError> {
    let mut owner = Owner {
        backend,
        keys: BTreeSet::new(),
        buttons: BTreeSet::new(),
        cleanup_failed,
        finished: false,
    };
    let result = (|| {
        if cancelled() || lease.snapshot().is_none() || !notify(Event::Ready) {
            return Ok(());
        }
        let mut last_packet = Instant::now();
        let mut last_activity = None;
        let mut host_active = false;
        while !cancelled() {
            if lease.snapshot().is_none() {
                let _ = notify(Event::Revoked);
                break;
            }
            let (activity, panic) = owner.backend.activity()?;
            if panic {
                let _ = notify(Event::Revoked);
                break;
            }
            if activity {
                last_activity = Some(Instant::now());
                owner.release()?;
                if !host_active {
                    host_active = true;
                    if !notify(Event::HostActive) {
                        break;
                    }
                }
            } else if host_active
                && last_activity.is_some_and(|at| at.elapsed() >= Duration::from_secs(2))
            {
                host_active = false;
                owner.backend.host_cursor(false)?;
                if !notify(Event::HostIdle) {
                    break;
                }
            }
            if last_packet.elapsed() >= Duration::from_secs(15) {
                let _ = notify(Event::Revoked);
                break;
            }
            match packets.recv_timeout(Duration::from_millis(5)) {
                Ok(Packet::Signal(Signal::Stop | Signal::Reset)) => break,
                Ok(packet) => {
                    if cancelled() {
                        break;
                    }
                    let Some(rect) = lease.snapshot() else {
                        break;
                    };
                    last_packet = Instant::now();
                    owner.input(&lease, rect, packet, host_active)?;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Ok(())
    })();
    owner.finish()?;
    result
}

pub fn run(
    lease: Arc<CaptureLease>,
    packets: mpsc::Receiver<Packet>,
    cancelled: impl Fn() -> bool,
    notify: impl FnMut(Event) -> bool,
) -> Result<(), InputError> {
    if cancelled() || lease.snapshot().is_none() {
        return Err(InputError::Unavailable);
    }
    #[cfg(windows)]
    {
        let failed = Arc::new(AtomicBool::new(false));
        let result = native::Native::open(&lease)
            .and_then(|backend| drive(backend, lease, packets, cancelled, notify, failed.clone()));
        if failed.load(Ordering::Acquire) || native::is_poisoned() {
            Err(InputError::Cleanup)
        } else {
            result
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (lease, packets, notify);
        Err(InputError::Unavailable)
    }
}

#[cfg(windows)]
mod native;
#[cfg(test)]
mod tests;
