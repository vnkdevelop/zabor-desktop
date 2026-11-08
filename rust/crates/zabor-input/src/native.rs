use super::*;
use std::cell::Cell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const SIGNATURE: usize = 0x5a41424f;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POISONED: AtomicBool = AtomicBool::new(false);
pub(super) fn is_poisoned() -> bool {
    POISONED.load(Ordering::Acquire)
}

#[derive(Clone, Copy, Default)]
struct Hooks {
    activity: bool,
    panic: bool,
    modifiers: u8,
    last_mouse: (i32, i32),
}

thread_local! { static HOOKS: Cell<Hooks> = Cell::new(Hooks::default()); }

unsafe extern "system" fn keyboard(code: i32, key: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 && data.0 != 0 {
        let event = unsafe { &*(data.0 as *const KBDLLHOOKSTRUCT) };
        if event.dwExtraInfo != SIGNATURE
            && event.flags.0 & (LLKHF_INJECTED.0 | LLKHF_LOWER_IL_INJECTED.0) == 0
        {
            HOOKS.with(|state| {
                let mut current = state.get();
                let down = matches!(key.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                let bit = match event.vkCode {
                    0x10 | 0xa0 | 0xa1 => 1,
                    0x11 | 0xa2 | 0xa3 => 2,
                    0x12 | 0xa4 | 0xa5 => 4,
                    0x5b | 0x5c => 8,
                    _ => 0,
                };
                if down {
                    current.modifiers |= bit;
                } else {
                    current.modifiers &= !bit;
                }
                current.activity |= down;
                current.panic |=
                    down && current.modifiers & 6 == 6 && matches!(event.vkCode, 0x08 | 0x1b);
                state.set(current);
            });
        }
    }
    unsafe { CallNextHookEx(None, code, key, data) }
}

unsafe extern "system" fn mouse(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 && data.0 != 0 {
        let event = unsafe { &*(data.0 as *const MSLLHOOKSTRUCT) };
        if event.dwExtraInfo != SIGNATURE
            && event.flags & (LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED) == 0
        {
            HOOKS.with(|state| {
                let mut current = state.get();
                let point = (event.pt.x, event.pt.y);
                let displacement = (i64::from(point.0) - i64::from(current.last_mouse.0))
                    .abs()
                    .max((i64::from(point.1) - i64::from(current.last_mouse.1)).abs());
                if message.0 as u32 != WM_MOUSEMOVE || displacement > 120 {
                    current.activity = true;
                    current.last_mouse = point;
                }
                state.set(current);
            });
        }
    }
    unsafe { CallNextHookEx(None, code, message, data) }
}

pub(super) struct Native {
    keyboard: Option<HHOOK>,
    mouse: Option<HHOOK>,
    window: Option<(u64, u32)>,
    hidden: bool,
    finished: bool,
}

impl Native {
    pub fn open(lease: &CaptureLease) -> Result<Self, InputError> {
        if POISONED.load(Ordering::Acquire) {
            return Err(InputError::Cleanup);
        }
        if ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(InputError::Unavailable);
        }
        let mut result = Self {
            keyboard: None,
            mouse: None,
            window: None,
            hidden: false,
            finished: false,
        };
        if let Some(window) = lease.window() {
            let process = window_process(window).ok_or(InputError::Unavailable)?;
            if process == std::process::id() {
                return Err(InputError::Unavailable);
            }
            result.window = Some((window, process));
        }
        let point = result.cursor()?;
        HOOKS.with(|state| {
            state.set(Hooks {
                last_mouse: point,
                ..Default::default()
            })
        });
        let module = unsafe { GetModuleHandleW(None) }.map_err(|_| InputError::Unavailable)?;
        result.keyboard = Some(
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), Some(module.into()), 0) }
                .map_err(|_| InputError::Unavailable)?,
        );
        result.mouse = Some(
            unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), Some(module.into()), 0) }
                .map_err(|_| InputError::Unavailable)?,
        );
        result.cursor_visible(false)?;
        Ok(result)
    }

    fn cursor_visible(&mut self, visible: bool) -> Result<(), InputError> {
        if visible {
            if self.hidden {
                unsafe {
                    SystemParametersInfoW(
                        SPI_SETCURSORS,
                        0,
                        None,
                        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                    )
                }
                .map_err(|_| InputError::Cleanup)?;
                self.hidden = false;
            }
        } else if !self.hidden {
            let and_mask = [255u8; 128];
            let xor_mask = [0u8; 128];
            self.hidden = true;
            for id in [
                32512, 32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645, 32646, 32648, 32649,
                32650,
            ] {
                let cursor = unsafe {
                    CreateCursor(
                        None,
                        0,
                        0,
                        32,
                        32,
                        and_mask.as_ptr().cast(),
                        xor_mask.as_ptr().cast(),
                    )
                }
                .map_err(|_| InputError::Unavailable)?;
                if unsafe { SetSystemCursor(cursor, SYSTEM_CURSOR_ID(id)) }.is_err() {
                    let _ = unsafe { DestroyCursor(cursor) };
                    return Err(InputError::Unavailable);
                }
            }
        }
        Ok(())
    }

    fn send(input: INPUT) -> Result<(), InputError> {
        if unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) } == 1 {
            Ok(())
        } else {
            Err(InputError::Unavailable)
        }
    }

    fn send_mouse(
        &mut self,
        flags: MOUSE_EVENT_FLAGS,
        data: u32,
        point: (i32, i32),
    ) -> Result<(), InputError> {
        Self::send(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: point.0,
                    dy: point.1,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: SIGNATURE,
                },
            },
        })
    }
}

fn window_process(window: u64) -> Option<u32> {
    let handle = HWND(usize::try_from(window).ok()? as *mut _);
    if !unsafe { IsWindow(Some(handle)) }.as_bool() {
        return None;
    }
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(handle, Some(&mut process)) };
    (process != 0).then_some(process)
}

impl Backend for Native {
    fn activity(&mut self) -> Result<(bool, bool), InputError> {
        let mut message = MSG::default();
        for _ in 0..64 {
            if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                return Err(InputError::Unavailable);
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let result = HOOKS.with(|state| {
            let mut current = state.get();
            let result = (current.activity, current.panic);
            current.activity = false;
            state.set(current);
            result
        });
        if result.0 || result.1 {
            self.cursor_visible(true)?;
        }
        Ok(result)
    }

    fn foreground(&self, lease: &CaptureLease) -> bool {
        match self.window {
            None => lease.window().is_none(),
            Some((window, process)) => {
                if lease.window() != Some(window) || window_process(window) != Some(process) {
                    return false;
                }
                let target = HWND(window as usize as *mut _);
                let foreground = unsafe { GetForegroundWindow() };
                !foreground.0.is_null()
                    && (foreground == target
                        || unsafe { GetAncestor(foreground, GA_ROOTOWNER) }
                            == unsafe { GetAncestor(target, GA_ROOTOWNER) })
            }
        }
    }

    fn cursor(&self) -> Result<(i32, i32), InputError> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }.map_err(|_| InputError::Unavailable)?;
        Ok((point.x, point.y))
    }

    fn move_to(&mut self, point: (i32, i32)) -> Result<(), InputError> {
        let desktop = Rect {
            left: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            top: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
            height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
        };
        let absolute = desktop.absolute(point).ok_or(InputError::Unavailable)?;
        self.send_mouse(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            0,
            absolute,
        )?;
        HOOKS.with(|state| {
            let mut current = state.get();
            current.last_mouse = point;
            state.set(current);
        });
        Ok(())
    }

    fn mouse(&mut self, button: u8, down: bool) -> Result<(), InputError> {
        let flags = match (button, down) {
            (0, true) => MOUSEEVENTF_LEFTDOWN,
            (0, false) => MOUSEEVENTF_LEFTUP,
            (1, true) => MOUSEEVENTF_MIDDLEDOWN,
            (1, false) => MOUSEEVENTF_MIDDLEUP,
            (2, true) => MOUSEEVENTF_RIGHTDOWN,
            (2, false) => MOUSEEVENTF_RIGHTUP,
            _ => return Err(InputError::Unavailable),
        };
        self.send_mouse(flags, 0, (0, 0))
    }

    fn wheel(&mut self, vertical: i16, horizontal: i16) -> Result<(), InputError> {
        if vertical != 0 {
            self.send_mouse(MOUSEEVENTF_WHEEL, (-i32::from(vertical)) as u32, (0, 0))?;
        }
        if horizontal != 0 {
            self.send_mouse(MOUSEEVENTF_HWHEEL, i32::from(horizontal) as u32, (0, 0))?;
        }
        Ok(())
    }

    fn key(&mut self, vk: u16, down: bool) -> Result<(), InputError> {
        let mut flags = if down {
            KEYBD_EVENT_FLAGS(0)
        } else {
            KEYEVENTF_KEYUP
        };
        if matches!(vk, 0x21..=0x28 | 0x2d | 0x2e | 0x6f | 0x90) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        Self::send(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: SIGNATURE,
                },
            },
        })
    }

    fn physical_modifiers(&self) -> u8 {
        let mut bits = HOOKS.with(|state| state.get().modifiers);
        for (key, bit) in [(0x10, 1), (0x11, 2), (0x12, 4), (0x5b, 8), (0x5c, 8)] {
            if unsafe { GetAsyncKeyState(key) } < 0 {
                bits |= bit;
            }
        }
        bits
    }

    fn host_cursor(&mut self, visible: bool) -> Result<(), InputError> {
        self.cursor_visible(visible)
    }

    fn poison(&mut self) {
        POISONED.store(true, Ordering::Release);
    }

    fn finish(&mut self) -> Result<(), InputError> {
        if self.finished {
            return Ok(());
        }
        let mut failed = self.cursor_visible(true).is_err();
        if let Some(hook) = self.keyboard {
            if unsafe { UnhookWindowsHookEx(hook) }.is_ok() {
                self.keyboard = None;
            } else {
                failed = true;
            }
        }
        if let Some(hook) = self.mouse {
            if unsafe { UnhookWindowsHookEx(hook) }.is_ok() {
                self.mouse = None;
            } else {
                failed = true;
            }
        }
        if failed {
            self.poison();
            return Err(InputError::Cleanup);
        }
        self.finished = true;
        ACTIVE.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}
