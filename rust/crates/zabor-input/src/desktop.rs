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

    pub fn acknowledge_at(&self, accepted_at: Instant) {
        if accepted_at > Instant::now() {
            self.revoke();
            return;
        }
        self.update(|state| {
            if state.acknowledged.is_none() {
                state.acknowledged = Some(accepted_at);
                state.activity = Some(accepted_at);
                state.confine = true;
            } else if state.acknowledged != Some(accepted_at) {
                self.revoke();
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

    pub fn is_alive(&self) -> bool {
        self.snapshot(Instant::now()).is_some()
    }

    fn update(&self, change: impl FnOnce(&mut ViewerState)) {
        match self.state.lock() {
            Ok(mut state) if self.live.load(Ordering::Acquire) => {
                if self.expired(&state, Instant::now()) {
                    self.revoke();
                } else {
                    change(&mut state);
                }
            }
            Ok(_) => {}
            Err(_) => self.revoke(),
        }
    }

    fn expired(&self, state: &ViewerState, now: Instant) -> bool {
        let Some(acknowledged) = state.acknowledged else {
            return now.saturating_duration_since(self.requested) >= INACTIVITY;
        };
        now.saturating_duration_since(state.activity.unwrap_or(acknowledged)) >= INACTIVITY
            || state.frame.is_some_and(|(_, _, at)| {
                at < acknowledged || now.saturating_duration_since(at) > FRAME_AGE
            })
    }

    fn snapshot(&self, now: Instant) -> Option<Option<Rect>> {
        let state = self.state.lock().ok()?;
        if !self.live.load(Ordering::Acquire) {
            return None;
        }
        if self.expired(&state, now) {
            self.revoke();
            return None;
        }
        let Some((viewport, pixels, _)) = state.frame else {
            return Some(None);
        };
        Some(
            state
                .confine
                .then_some(fitted_video_rect(viewport, pixels)?),
        )
    }
}

struct ViewerRetirement<'a> {
    lease: &'a ViewerCursorLease,
    retired: &'a dyn Fn(),
}

impl ViewerRetirement<'_> {
    fn retire(&self) {
        self.lease.revoke();
        (self.retired)();
    }
}

impl Drop for ViewerRetirement<'_> {
    fn drop(&mut self) {
        self.retire();
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

#[cfg(test)]
fn drive_cursor<B: CursorBackend>(
    backend: B,
    lease: &ViewerCursorLease,
    cancelled: impl Fn() -> bool,
    observation: CleanupObservation,
    wait: impl FnMut(),
) -> Result<(), InputError> {
    drive_cursor_with_retirement(backend, lease, cancelled, observation, wait, &|| {})
}

fn drive_cursor_with_retirement<B: CursorBackend>(
    backend: B,
    lease: &ViewerCursorLease,
    cancelled: impl Fn() -> bool,
    observation: CleanupObservation,
    mut wait: impl FnMut(),
    retired: &dyn Fn(),
) -> Result<(), InputError> {
    let mut owner = CursorOwner::acquire(backend, observation.clone())?;
    let retirement = ViewerRetirement { lease, retired };
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
    retirement.retire();
    owner.finish()?;
    result
}

pub fn run_viewer_cursor(
    lease: Arc<ViewerCursorLease>,
    cancelled: impl Fn() -> bool,
    observation: CleanupObservation,
) -> Result<(), InputError> {
    run_viewer_cursor_with_retirement(lease, cancelled, observation, || {})
}

pub fn run_viewer_cursor_with_retirement(
    lease: Arc<ViewerCursorLease>,
    cancelled: impl Fn() -> bool,
    observation: CleanupObservation,
    retired: impl Fn(),
) -> Result<(), InputError> {
    let retirement = ViewerRetirement {
        lease: &lease,
        retired: &retired,
    };
    let result = if cancelled() || !lease.live.load(Ordering::Acquire) {
        Ok(())
    } else if lease.snapshot(Instant::now()).flatten().is_none() {
        Err(InputError::Unavailable)
    } else {
        #[cfg(windows)]
        {
            native::Cursor::open(lease.window, observation.clone()).and_then(|backend| {
                drive_cursor_with_retirement(
                    backend,
                    &lease,
                    cancelled,
                    observation.clone(),
                    || {
                        std::thread::sleep(Duration::from_millis(5));
                    },
                    &retired,
                )
            })
        }
        #[cfg(not(windows))]
        {
            Err(InputError::Unavailable)
        }
    };
    retirement.retire();
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
pub fn passive_host_style(style: u32) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    };
    (style | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT).0) & !WS_EX_APPWINDOW.0
}

#[cfg(windows)]
fn passive_position_flags(
    flags: windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS,
) -> windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS {
    flags | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
}

#[cfg(windows)]
mod passive {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use windows::core::w;
    use windows::Win32::Foundation::{
        GetLastError, SetLastError, HANDLE, HWND, LPARAM, LRESULT, WIN32_ERROR, WPARAM,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::*;

    thread_local! {
        static WINDOWS: RefCell<BTreeMap<usize, Rc<State>>> = const { RefCell::new(BTreeMap::new()) };
    }

    fn identity(window: HWND) -> Option<(u32, u32)> {
        if !unsafe { IsWindow(Some(window)) }.as_bool() {
            return None;
        }
        let mut process = 0;
        let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
        (process == std::process::id() && thread == unsafe { GetCurrentThreadId() })
            .then_some((process, thread))
    }

    struct State {
        window: HWND,
        identity: (u32, u32),
        host_cookie: HANDLE,
        original: WNDPROC,
        installed: Cell<bool>,
    }

    impl State {
        fn owned(&self) -> bool {
            identity(self.window) == Some(self.identity)
                && std::ptr::eq(
                    unsafe { GetPropW(self.window, w!("Zabor.NativeRc.PassiveHandoff")) }
                        .0
                        .cast::<Self>(),
                    self,
                )
        }

        fn current(&self) -> bool {
            self.owned()
                && unsafe { GetPropW(self.window, w!("Zabor.NativeRc.HostSurfaceLease")) }
                    == self.host_cookie
        }
    }

    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state = WINDOWS.with(|windows| windows.borrow().get(&(window.0 as usize)).cloned());
        let Some(state) = state else {
            return unsafe { DefWindowProcW(window, message, wparam, lparam) };
        };
        if state.current() && message == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        let result = unsafe { CallWindowProcW(state.original, window, message, wparam, lparam) };
        if state.current() {
            if message == WM_STYLECHANGING && wparam.0 as isize == GWL_EXSTYLE.0 as isize {
                if let Some(style) = unsafe { (lparam.0 as *mut STYLESTRUCT).as_mut() } {
                    style.styleNew = passive_host_style(style.styleNew);
                }
            } else if message == WM_WINDOWPOSCHANGING {
                if let Some(position) = unsafe { (lparam.0 as *mut WINDOWPOS).as_mut() } {
                    position.flags = passive_position_flags(position.flags);
                }
            }
        }
        if message == WM_NCDESTROY {
            WINDOWS.with(|windows| {
                windows.borrow_mut().remove(&(window.0 as usize));
            });
        }
        result
    }

    fn set_long(
        window: HWND,
        index: WINDOW_LONG_PTR_INDEX,
        value: isize,
    ) -> Result<isize, InputError> {
        unsafe { SetLastError(WIN32_ERROR(0)) };
        let previous = unsafe { SetWindowLongPtrW(window, index, value) };
        if previous == 0 && unsafe { GetLastError() }.0 != 0 {
            Err(InputError::Cleanup)
        } else {
            Ok(previous)
        }
    }

    pub struct PassiveHostWindow {
        state: Option<Rc<State>>,
        observation: CleanupObservation,
        affinity: PhantomData<Rc<()>>,
    }

    impl PassiveHostWindow {
        pub fn attach_hidden(
            window: u64,
            observation: CleanupObservation,
        ) -> Result<Self, InputError> {
            let window =
                HWND(usize::try_from(window).map_err(|_| InputError::Unavailable)? as *mut _);
            let identity = identity(window).ok_or(InputError::Unavailable)?;
            let host_cookie = unsafe { GetPropW(window, w!("Zabor.NativeRc.HostSurfaceLease")) };
            if host_cookie.0.is_null()
                || unsafe { IsWindowVisible(window) }.as_bool()
                || !unsafe { GetPropW(window, w!("Zabor.NativeRc.PassiveHandoff")) }
                    .0
                    .is_null()
            {
                return Err(InputError::Unavailable);
            }
            let original = unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) };
            if original == 0 {
                return Err(InputError::Unavailable);
            }
            let state = Rc::new(State {
                window,
                identity,
                host_cookie,
                original: unsafe { std::mem::transmute::<isize, WNDPROC>(original) },
                installed: Cell::new(false),
            });
            unsafe {
                SetPropW(
                    window,
                    w!("Zabor.NativeRc.PassiveHandoff"),
                    Some(HANDLE((&*state as *const State).cast_mut().cast())),
                )
            }
            .map_err(|_| InputError::Unavailable)?;
            WINDOWS.with(|windows| {
                windows
                    .borrow_mut()
                    .insert(window.0 as usize, state.clone());
            });
            let mut owner = Self {
                state: Some(state),
                observation,
                affinity: PhantomData,
            };
            let state = owner.state.as_ref().unwrap();
            let previous = set_long(window, GWLP_WNDPROC, procedure as *const () as isize)?;
            state.installed.set(true);
            if previous != original {
                owner.observation.complete(Err(InputError::Cleanup));
                return Err(InputError::Cleanup);
            }
            owner.reassert()?;
            Ok(owner)
        }

        pub fn reassert(&mut self) -> Result<(), InputError> {
            let state = self.state.as_ref().ok_or(InputError::Unavailable)?;
            if !state.current()
                || unsafe { GetWindowLongPtrW(state.window, GWLP_WNDPROC) }
                    != procedure as *const () as isize
            {
                return Err(InputError::Unavailable);
            }
            let style = unsafe { GetWindowLongPtrW(state.window, GWL_EXSTYLE) } as u32;
            set_long(
                state.window,
                GWL_EXSTYLE,
                passive_host_style(style) as isize,
            )?;
            self.verify()
        }

        pub fn verify(&self) -> Result<(), InputError> {
            let state = self.state.as_ref().ok_or(InputError::Unavailable)?;
            let style = unsafe { GetWindowLongPtrW(state.window, GWL_EXSTYLE) } as u32;
            if state.current()
                && unsafe { GetWindowLongPtrW(state.window, GWLP_WNDPROC) }
                    == procedure as *const () as isize
                && style == passive_host_style(style)
            {
                Ok(())
            } else {
                Err(InputError::Unavailable)
            }
        }

        pub fn finish(&mut self) -> Result<(), InputError> {
            let Some(state) = &self.state else {
                return Ok(());
            };
            let result = (|| {
                if !unsafe { IsWindow(Some(state.window)) }.as_bool() {
                    return Ok(());
                }
                if !state.owned() {
                    return Err(InputError::Cleanup);
                }
                if state.installed.get() {
                    if unsafe { GetWindowLongPtrW(state.window, GWLP_WNDPROC) }
                        != procedure as *const () as isize
                    {
                        return Err(InputError::Cleanup);
                    }
                    let original = state
                        .original
                        .map_or(0, |original| original as *const () as isize);
                    if set_long(state.window, GWLP_WNDPROC, original)?
                        != procedure as *const () as isize
                    {
                        return Err(InputError::Cleanup);
                    }
                    state.installed.set(false);
                }
                let removed =
                    unsafe { RemovePropW(state.window, w!("Zabor.NativeRc.PassiveHandoff")) }
                        .map_err(|_| InputError::Cleanup)?;
                if !std::ptr::eq(removed.0.cast::<State>(), Rc::as_ptr(state)) {
                    return Err(InputError::Cleanup);
                }
                Ok(())
            })();
            self.observation.complete(result);
            if result.is_ok() {
                let window = state.window.0 as usize;
                WINDOWS.with(|windows| {
                    windows.borrow_mut().remove(&window);
                });
                self.state = None;
            }
            result
        }
    }

    impl Drop for PassiveHostWindow {
        fn drop(&mut self) {
            let _ = self.finish();
        }
    }
}

#[cfg(windows)]
pub use passive::PassiveHostWindow;

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::HostSurface;
#[cfg(test)]
mod tests;
