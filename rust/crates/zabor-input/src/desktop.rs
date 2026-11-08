use crate::{InputError, Rect};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FRAME_AGE: Duration = Duration::from_millis(750);
const INACTIVITY: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupState {
    Pending,
    Restored,
    Failed,
    Superseded,
}

#[derive(Clone, Default)]
pub struct CleanupObservation(Arc<AtomicU8>);

impl CleanupObservation {
    pub fn state(&self) -> CleanupState {
        match self.0.load(Ordering::Acquire) {
            0 => CleanupState::Pending,
            1 => CleanupState::Restored,
            3 => CleanupState::Superseded,
            _ => CleanupState::Failed,
        }
    }

    fn complete(&self, result: Result<(), InputError>) {
        if result.is_err() {
            self.0.store(2, Ordering::Release);
        } else {
            let _ = self
                .0
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    fn superseded(&self) {
        let _ = self
            .0
            .compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub fn fitted_video_rect(viewport: Rect, pixels: (u32, u32)) -> Option<Rect> {
    if !viewport.valid() || pixels.0 == 0 || pixels.1 == 0 {
        return None;
    }
    let (w, h) = (i64::from(viewport.width), i64::from(viewport.height));
    let (pw, ph) = (i64::from(pixels.0), i64::from(pixels.1));
    let (left, top, right, bottom) = if w * ph <= h * pw {
        let gap = h * pw - w * ph;
        (
            0,
            (gap + 2 * pw - 1) / (2 * pw),
            w,
            (h * pw + w * ph) / (2 * pw),
        )
    } else {
        let gap = w * ph - h * pw;
        (
            (gap + 2 * ph - 1) / (2 * ph),
            0,
            (w * ph + h * pw) / (2 * ph),
            h,
        )
    };
    let rect = Rect {
        left: viewport.left.checked_add(i32::try_from(left).ok()?)?,
        top: viewport.top.checked_add(i32::try_from(top).ok()?)?,
        width: i32::try_from(right - left).ok()?,
        height: i32::try_from(bottom - top).ok()?,
    };
    rect.valid().then_some(rect)
}

pub fn physical_viewport(bounds: [f64; 4], scale: f64) -> Option<Rect> {
    let [left, top, width, height] = bounds;
    if !bounds.iter().all(|value| value.is_finite())
        || !scale.is_finite()
        || scale <= 0.0
        || width <= 0.0
        || height <= 0.0
    {
        return None;
    }
    let edges = [
        (left * scale).ceil(),
        (top * scale).ceil(),
        ((left + width) * scale).floor(),
        ((top + height) * scale).floor(),
    ];
    if !edges.iter().all(|value| {
        value.is_finite() && *value >= f64::from(i32::MIN) && *value <= f64::from(i32::MAX)
    }) {
        return None;
    }
    let [left, top, right, bottom] = edges.map(|value| value as i32);
    let rect = Rect {
        left,
        top,
        width: right.checked_sub(left)?,
        height: bottom.checked_sub(top)?,
    };
    rect.valid().then_some(rect)
}

pub fn host_pill_bounds(capture: Rect, size: (i32, i32)) -> Option<Rect> {
    if !capture.valid()
        || size.0 <= 0
        || size.1 <= 0
        || size.0 > capture.width
        || i64::from(size.1) + 10 > i64::from(capture.height)
    {
        return None;
    }
    let bounds = Rect {
        left: capture.left.checked_add((capture.width - size.0) / 2)?,
        top: capture.top.checked_add(10)?,
        width: size.0,
        height: size.1,
    };
    contains_rect(capture, bounds).then_some(bounds)
}

#[derive(Default)]
struct ViewerState {
    acknowledged: Option<Instant>,
    frame: Option<(Rect, (u32, u32), Instant)>,
    activity: Option<Instant>,
    confine: bool,
}

pub struct ViewerCursorLease {
    window: u64,
    requested: Instant,
    live: AtomicBool,
    state: Mutex<ViewerState>,
}

impl ViewerCursorLease {
    pub fn new(window: u64) -> Arc<Self> {
        Arc::new(Self {
            window,
            requested: Instant::now(),
            live: AtomicBool::new(window != 0),
            state: Mutex::new(ViewerState::default()),
        })
    }

    pub fn acknowledge(&self) {
        let now = Instant::now();
        self.update(|state| {
            if state.acknowledged.is_none() {
                state.acknowledged = Some(now);
                state.activity = Some(now);
                state.confine = true;
            }
        });
    }

    pub fn decoded_p2p_frame(&self, viewport: Rect, pixels: (u32, u32), decoded_at: Instant) {
        if fitted_video_rect(viewport, pixels).is_none() {
            self.revoke();
            return;
        }
        self.update(|state| {
            if state.acknowledged.is_some_and(|at| decoded_at >= at)
                && decoded_at <= Instant::now()
                && decoded_at.elapsed() <= FRAME_AGE
                && state.frame.is_none_or(|(_, _, at)| decoded_at >= at)
            {
                state.frame = Some((viewport, pixels, decoded_at));
            }
        });
    }

    pub fn relayout(&self, viewport: Rect) {
        self.update(|state| {
            if let Some((_, pixels, at)) = state.frame {
                state.frame = Some((viewport, pixels, at));
            }
        });
    }

    pub fn clear_frame(&self) {
        self.revoke();
    }

    pub fn activity(&self) {
        self.update(|state| state.activity = Some(Instant::now()));
    }

    pub fn set_confined(&self, confine: bool) {
        self.update(|state| state.confine = confine);
    }

    pub fn revoke(&self) {
        self.live.store(false, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.snapshot(Instant::now()).flatten().is_some()
    }

    fn update(&self, change: impl FnOnce(&mut ViewerState)) {
        match self.state.lock() {
            Ok(mut state) if self.live.load(Ordering::Acquire) => change(&mut state),
            Ok(_) => {}
            Err(_) => self.revoke(),
        }
    }

    fn snapshot(&self, now: Instant) -> Option<Option<Rect>> {
        let state = self.state.lock().ok()?;
        if !self.live.load(Ordering::Acquire) {
            return None;
        }
        let Some(acknowledged) = state.acknowledged else {
            return (now.saturating_duration_since(self.requested) < INACTIVITY).then_some(None);
        };
        if now.saturating_duration_since(state.activity.unwrap_or(acknowledged)) >= INACTIVITY {
            return None;
        }
        let Some((viewport, pixels, at)) = state.frame else {
            return Some(None);
        };
        if at < acknowledged || now.saturating_duration_since(at) > FRAME_AGE {
            return None;
        }
        Some(
            state
                .confine
                .then_some(fitted_video_rect(viewport, pixels)?),
        )
    }
}

struct ViewerRetirement<'a>(&'a ViewerCursorLease);

impl Drop for ViewerRetirement<'_> {
    fn drop(&mut self) {
        self.0.revoke();
    }
}

trait CursorBackend {
    fn foreground(&self) -> bool;
    fn screen_rect(&self, client: Rect) -> Result<Rect, InputError>;
    fn clipping(&self) -> Result<Rect, InputError>;
    fn desktop(&self) -> Result<Rect, InputError>;
    fn clip(&mut self, rect: Option<Rect>) -> Result<(), InputError>;
    fn poison(&mut self);
    fn finish(&mut self) -> Result<(), InputError>;
}

struct CursorOwner<B: CursorBackend> {
    backend: B,
    original: Option<Rect>,
    baseline: Rect,
    owned: Option<Rect>,
    observation: CleanupObservation,
    finished: bool,
    affinity: PhantomData<Rc<()>>,
}

impl<B: CursorBackend> CursorOwner<B> {
    fn acquire(backend: B, observation: CleanupObservation) -> Result<Self, InputError> {
        let clipping = backend.clipping()?;
        let desktop = backend.desktop()?;
        Ok(Self {
            backend,
            original: (clipping != desktop).then_some(clipping),
            baseline: clipping,
            owned: None,
            observation,
            finished: false,
            affinity: PhantomData,
        })
    }

    fn apply(&mut self, rect: Option<Rect>) -> Result<bool, InputError> {
        if !self.backend.foreground() {
            return Ok(false);
        }
        if self.backend.clipping()? != self.owned.unwrap_or(self.baseline) {
            self.owned = None;
            self.observation.superseded();
            return Ok(false);
        }
        match rect {
            Some(client) => {
                let screen = self.backend.screen_rect(client)?;
                if let Some(original) = self.original {
                    if !contains_rect(original, screen) {
                        return Err(InputError::Unavailable);
                    }
                }
                if self.owned != Some(screen) {
                    self.owned = Some(screen);
                    self.backend.clip(Some(screen))?;
                }
            }
            None => self.restore()?,
        }
        Ok(true)
    }

    fn restore(&mut self) -> Result<(), InputError> {
        if let Some(owned) = self.owned {
            if self.backend.clipping()? == owned {
                self.backend.clip(self.original)?;
            } else {
                self.observation.superseded();
            }
            self.owned = None;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), InputError> {
        if self.finished {
            return Ok(());
        }
        let restored = self.restore();
        if restored.is_err() {
            self.backend.poison();
        }
        let finished = self.backend.finish();
        let result = if restored.is_err() || finished.is_err() {
            Err(InputError::Cleanup)
        } else {
            Ok(())
        };
        self.observation.complete(result);
        if result.is_err() {
            self.backend.poison();
        } else {
            self.finished = true;
        }
        result
    }
}

impl<B: CursorBackend> Drop for CursorOwner<B> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn contains_rect(outer: Rect, inner: Rect) -> bool {
    outer.valid()
        && inner.valid()
        && inner.left >= outer.left
        && inner.top >= outer.top
        && i64::from(inner.left) + i64::from(inner.width)
            <= i64::from(outer.left) + i64::from(outer.width)
        && i64::from(inner.top) + i64::from(inner.height)
            <= i64::from(outer.top) + i64::from(outer.height)
}

fn drive_cursor<B: CursorBackend>(
    backend: B,
    lease: &ViewerCursorLease,
    cancelled: impl Fn() -> bool,
    observation: CleanupObservation,
    mut wait: impl FnMut(),
) -> Result<(), InputError> {
    let mut owner = CursorOwner::acquire(backend, observation.clone())?;
    let _retirement = ViewerRetirement(lease);
    let result = (|| {
        while !cancelled() {
            let Some(rect) = lease.snapshot(Instant::now()) else {
                break;
            };
            if cancelled() || !owner.apply(rect)? {
                break;
            }
            wait();
        }
        Ok(())
    })();
    lease.revoke();
    owner.finish()?;
    result
}

pub fn run_viewer_cursor(
    lease: Arc<ViewerCursorLease>,
    cancelled: impl Fn() -> bool,
    observation: CleanupObservation,
) -> Result<(), InputError> {
    let _retirement = ViewerRetirement(&lease);
    let result = if cancelled() || !lease.live.load(Ordering::Acquire) {
        Ok(())
    } else if lease.snapshot(Instant::now()).flatten().is_none() {
        Err(InputError::Unavailable)
    } else {
        #[cfg(windows)]
        {
            native::Cursor::open(lease.window, observation.clone()).and_then(|backend| {
                drive_cursor(backend, &lease, cancelled, observation.clone(), || {
                    std::thread::sleep(Duration::from_millis(5));
                })
            })
        }
        #[cfg(not(windows))]
        {
            Err(InputError::Unavailable)
        }
    };
    lease.revoke();
    let result = if observation.state() == CleanupState::Failed {
        Err(InputError::Cleanup)
    } else {
        result
    };
    observation.complete(if result == Err(InputError::Cleanup) {
        Err(InputError::Cleanup)
    } else {
        Ok(())
    });
    result
}

trait SurfaceBackend {
    fn current(&self) -> bool;
    fn present(&mut self, bounds: Rect) -> Result<(), InputError>;
    fn restore(&mut self) -> Result<(), InputError>;
}

struct SurfaceOwner<B: SurfaceBackend> {
    backend: B,
    observation: CleanupObservation,
    finished: bool,
    affinity: PhantomData<Rc<()>>,
}

impl<B: SurfaceBackend> SurfaceOwner<B> {
    fn present(&mut self, bounds: Rect, authorized: bool) -> Result<(), InputError> {
        if self.finished {
            return Err(InputError::Unavailable);
        }
        if !authorized || !self.backend.current() || !bounds.valid() {
            self.finish()?;
            return Err(InputError::Unavailable);
        }
        let result = self.backend.present(bounds);
        if result.is_err() {
            self.finish()?;
        }
        result
    }

    fn finish(&mut self) -> Result<(), InputError> {
        if self.finished {
            return Ok(());
        }
        let result = self.backend.restore().map_err(|_| InputError::Cleanup);
        self.observation.complete(result);
        self.finished = result.is_ok();
        result
    }
}

impl<B: SurfaceBackend> Drop for SurfaceOwner<B> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::HostSurface;
#[cfg(test)]
mod tests;
