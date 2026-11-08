use super::*;
use crate::CaptureLease;
use std::sync::atomic::AtomicUsize;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, SetLastError, HANDLE, HWND, POINT, RECT, WIN32_ERROR,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;

static CURSOR_ACTIVE: AtomicBool = AtomicBool::new(false);
static CURSOR_POISONED: AtomicBool = AtomicBool::new(false);
static NEXT_ANCHOR: AtomicUsize = AtomicUsize::new(1);

fn identity(window: HWND) -> Option<(u32, u32)> {
    if !unsafe { IsWindow(Some(window)) }.as_bool() {
        return None;
    }
    let mut process = 0;
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
    (process == std::process::id() && thread != 0).then_some((process, thread))
}

fn handle(window: u64) -> Result<HWND, InputError> {
    let handle = HWND(usize::try_from(window).map_err(|_| InputError::Unavailable)? as *mut _);
    identity(handle).ok_or(InputError::Unavailable)?;
    Ok(handle)
}

struct WindowAnchor {
    window: HWND,
    identity: (u32, u32),
    property: PCWSTR,
    cookie: HANDLE,
    finished: bool,
}

impl WindowAnchor {
    fn open(window: u64, property: PCWSTR) -> Result<Self, InputError> {
        let window = handle(window)?;
        if !unsafe { GetPropW(window, property) }.0.is_null() {
            return Err(InputError::Unavailable);
        }
        let mut cookie = NEXT_ANCHOR.load(Ordering::Acquire);
        loop {
            let next = cookie.checked_add(1).ok_or(InputError::Unavailable)?;
            match NEXT_ANCHOR.compare_exchange_weak(
                cookie,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(current) => cookie = current,
            }
        }
        let cookie = HANDLE(cookie as *mut _);
        let identity = identity(window).ok_or(InputError::Unavailable)?;
        unsafe { SetPropW(window, property, Some(cookie)) }.map_err(|_| InputError::Unavailable)?;
        Ok(Self {
            window,
            identity,
            property,
            cookie,
            finished: false,
        })
    }

    fn current(&self) -> bool {
        !self.finished
            && identity(self.window) == Some(self.identity)
            && unsafe { GetPropW(self.window, self.property) } == self.cookie
    }

    fn finish(&mut self) -> Result<(), InputError> {
        if self.current() {
            let removed = unsafe { RemovePropW(self.window, self.property) }
                .map_err(|_| InputError::Cleanup)?;
            if removed != self.cookie {
                return Err(InputError::Cleanup);
            }
        }
        self.finished = true;
        Ok(())
    }
}

fn rect(value: RECT) -> Result<Rect, InputError> {
    let value = Rect {
        left: value.left,
        top: value.top,
        width: value
            .right
            .checked_sub(value.left)
            .ok_or(InputError::Unavailable)?,
        height: value
            .bottom
            .checked_sub(value.top)
            .ok_or(InputError::Unavailable)?,
    };
    value
        .valid()
        .then_some(value)
        .ok_or(InputError::Unavailable)
}

fn native_rect(value: Rect) -> RECT {
    RECT {
        left: value.left,
        top: value.top,
        right: value.left + value.width,
        bottom: value.top + value.height,
    }
}

pub(super) struct Cursor {
    anchor: WindowAnchor,
    observation: CleanupObservation,
    finished: bool,
    affinity: PhantomData<Rc<()>>,
}

impl Cursor {
    pub(super) fn open(window: u64, observation: CleanupObservation) -> Result<Self, InputError> {
        if CURSOR_POISONED.load(Ordering::Acquire) {
            return Err(InputError::Cleanup);
        }
        if CURSOR_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(InputError::Unavailable);
        }
        let anchor = match WindowAnchor::open(window, w!("Zabor.NativeRc.CursorLease")) {
            Ok(anchor) => anchor,
            Err(error) => {
                CURSOR_ACTIVE.store(false, Ordering::Release);
                return Err(error);
            }
        };
        Ok(Self {
            anchor,
            observation,
            finished: false,
            affinity: PhantomData,
        })
    }
}

impl CursorBackend for Cursor {
    fn foreground(&self) -> bool {
        self.anchor.current()
            && unsafe { GetForegroundWindow() } == self.anchor.window
            && !unsafe { IsIconic(self.anchor.window) }.as_bool()
    }

    fn screen_rect(&self, client: Rect) -> Result<Rect, InputError> {
        if !self.foreground() {
            return Err(InputError::Unavailable);
        }
        let mut bounds = RECT::default();
        unsafe { GetClientRect(self.anchor.window, &mut bounds) }
            .map_err(|_| InputError::Unavailable)?;
        if !contains_rect(rect(bounds)?, client) {
            return Err(InputError::Unavailable);
        }
        let mut point = POINT {
            x: client.left,
            y: client.top,
        };
        if !unsafe { ClientToScreen(self.anchor.window, &mut point) }.as_bool() {
            return Err(InputError::Unavailable);
        }
        let screen = Rect {
            left: point.x,
            top: point.y,
            ..client
        };
        if !contains_rect(self.desktop()?, screen) {
            return Err(InputError::Unavailable);
        }
        Ok(screen)
    }

    fn clipping(&self) -> Result<Rect, InputError> {
        let mut value = RECT::default();
        unsafe { GetClipCursor(&mut value) }.map_err(|_| InputError::Unavailable)?;
        rect(value)
    }

    fn desktop(&self) -> Result<Rect, InputError> {
        let value = Rect {
            left: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            top: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
            height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
        };
        value
            .valid()
            .then_some(value)
            .ok_or(InputError::Unavailable)
    }

    fn clip(&mut self, value: Option<Rect>) -> Result<(), InputError> {
        let value = value.map(native_rect);
        unsafe { ClipCursor(value.as_ref().map(|value| value as *const RECT)) }
            .map_err(|_| InputError::Unavailable)
    }

    fn poison(&mut self) {
        CURSOR_POISONED.store(true, Ordering::Release);
    }

    fn finish(&mut self) -> Result<(), InputError> {
        if self.finished {
            return Ok(());
        }
        let result = self.anchor.finish();
        if result.is_err() {
            self.poison();
            self.observation.complete(Err(InputError::Cleanup));
        } else {
            self.finished = true;
            if !CURSOR_POISONED.load(Ordering::Acquire) {
                CURSOR_ACTIVE.store(false, Ordering::Release);
            }
        }
        result
    }
}

impl Drop for Cursor {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

pub struct HostSurface {
    owner: SurfaceOwner<NativeSurface>,
    capture: Arc<CaptureLease>,
}

struct NativeSurface {
    anchor: WindowAnchor,
    style: isize,
}

fn set_style(window: HWND, value: isize) -> Result<(), InputError> {
    unsafe { SetLastError(WIN32_ERROR(0)) };
    let previous = unsafe { SetWindowLongPtrW(window, GWL_EXSTYLE, value) };
    if previous == 0 && unsafe { GetLastError() }.0 != 0 {
        Err(InputError::Unavailable)
    } else {
        Ok(())
    }
}

impl HostSurface {
    pub fn attach_hidden(
        window: u64,
        capture: Arc<CaptureLease>,
        observation: CleanupObservation,
    ) -> Result<Self, InputError> {
        let result = Self::attach(window, capture, observation.clone());
        if observation.state() == CleanupState::Failed {
            Err(InputError::Cleanup)
        } else {
            if result.is_err() {
                observation.complete(Ok(()));
            }
            result
        }
    }

    fn attach(
        window: u64,
        capture: Arc<CaptureLease>,
        observation: CleanupObservation,
    ) -> Result<Self, InputError> {
        let window = handle(window)?;
        if unsafe { IsWindowVisible(window) }.as_bool()
            || capture.snapshot().is_none()
            || identity(window).map(|(_, thread)| thread) != Some(unsafe { GetCurrentThreadId() })
        {
            return Err(InputError::Unavailable);
        }
        let anchor = WindowAnchor::open(
            window.0 as usize as u64,
            w!("Zabor.NativeRc.HostSurfaceLease"),
        )?;
        let mut surface = Self {
            owner: SurfaceOwner {
                backend: NativeSurface {
                    anchor,
                    style: unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) },
                },
                observation,
                finished: false,
                affinity: PhantomData,
            },
            capture,
        };
        let style = (surface.owner.backend.style
            | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT).0 as isize)
            & !(WS_EX_APPWINDOW.0 as isize);
        set_style(window, style)?;
        if surface.capture.snapshot().is_none() {
            surface.finish()?;
            return Err(InputError::Unavailable);
        }
        Ok(surface)
    }

    pub fn present(
        &mut self,
        bounds: Rect,
        authorized: impl FnOnce() -> bool,
    ) -> Result<(), InputError> {
        self.owner
            .present(bounds, self.capture.snapshot().is_some() && authorized())
    }

    pub fn finish(&mut self) -> Result<(), InputError> {
        self.owner.finish()
    }
}

impl SurfaceBackend for NativeSurface {
    fn current(&self) -> bool {
        self.anchor.current()
    }

    fn present(&mut self, bounds: Rect) -> Result<(), InputError> {
        unsafe {
            SetWindowPos(
                self.anchor.window,
                Some(HWND_TOPMOST),
                bounds.left,
                bounds.top,
                bounds.width,
                bounds.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            )
        }
        .map_err(|_| InputError::Unavailable)
    }

    fn restore(&mut self) -> Result<(), InputError> {
        let result = if !self.anchor.current() {
            Ok(())
        } else {
            let hidden = unsafe {
                SetWindowPos(
                    self.anchor.window,
                    Some(if self.style & WS_EX_TOPMOST.0 as isize != 0 {
                        HWND_TOPMOST
                    } else {
                        HWND_NOTOPMOST
                    }),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE,
                )
            };
            let style = set_style(self.anchor.window, self.style);
            if hidden.is_err() || style.is_err() {
                Err(InputError::Cleanup)
            } else {
                Ok(())
            }
        };
        let anchor = if result.is_ok() {
            self.anchor.finish()
        } else {
            Ok(())
        };
        if result.is_err() || anchor.is_err() {
            Err(InputError::Cleanup)
        } else {
            Ok(())
        }
    }
}
