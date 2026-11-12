use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const CALLBACK: u32 = WM_APP + 37;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const RESTORE: usize = 1;
const QUIT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Restore,
    Quit,
    Unavailable,
}

struct State {
    icon: HICON,
    ready: Cell<bool>,
    taskbar_created: u32,
    labels: RefCell<(Vec<u16>, Vec<u16>)>,
    event: Box<dyn Fn(Event)>,
}

pub struct Tray {
    window: HWND,
    state: Box<State>,
    thread_bound: PhantomData<Rc<()>>,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

const ICON_SIZE: usize = 16;
const CORNER_RADIUS: i32 = 3;

fn corner_distance(value: usize) -> i32 {
    if value < CORNER_RADIUS as usize {
        CORNER_RADIUS - 1 - value as i32
    } else if value >= ICON_SIZE - CORNER_RADIUS as usize {
        value as i32 - (ICON_SIZE as i32 - CORNER_RADIUS)
    } else {
        -1
    }
}

fn icon_pixel(x: usize, y: usize) -> Option<[u8; 4]> {
    let (dx, dy) = (corner_distance(x), corner_distance(y));
    if dx >= 0 && dy >= 0 && dx * dx + dy * dy > CORNER_RADIUS * CORNER_RADIUS {
        return None;
    }
    let bar = |row: usize| y == row && (4..=11).contains(&x);
    let diagonal = (6..=9).contains(&y) && {
        let start = 11 - (y as i32 - 6) * 2;
        x as i32 == start || x as i32 + 1 == start
    };
    let glyph = bar(4) || bar(5) || bar(10) || bar(11) || diagonal;
    Some(if glyph {
        [255, 255, 255, 255]
    } else {
        [112, 30, 200, 255]
    })
}

fn create_icon() -> windows::core::Result<HICON> {
    let mut xor = [0_u8; ICON_SIZE * ICON_SIZE * 4];
    for row in 0..ICON_SIZE {
        let y = ICON_SIZE - 1 - row;
        for x in 0..ICON_SIZE {
            let color = icon_pixel(x, y).unwrap_or([0, 0, 0, 0]);
            let offset = (row * ICON_SIZE + x) * 4;
            xor[offset..offset + 4].copy_from_slice(&color);
        }
    }
    let and = [0_u8; ICON_SIZE * (ICON_SIZE / 8)];
    unsafe {
        CreateIcon(
            None,
            ICON_SIZE as i32,
            ICON_SIZE as i32,
            1,
            32,
            and.as_ptr(),
            xor.as_ptr(),
        )
    }
}

fn data(window: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: CALLBACK,
        hIcon: icon,
        ..Default::default()
    };
    data.szTip[..6].copy_from_slice(&[122, 97, 98, 111, 114, 0]);
    data
}

fn add(window: HWND, state: &State) -> bool {
    let mut icon = data(window, state.icon);
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &icon) }.as_bool() {
        state.ready.set(false);
        return false;
    }
    icon.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let ready = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &icon) }.as_bool();
    state.ready.set(ready);
    if !ready {
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &icon) };
    }
    ready
}

fn menu(window: HWND, state: &State) -> windows::core::Result<()> {
    let menu = unsafe { CreatePopupMenu() }?;
    let result = (|| {
        let labels = state.labels.borrow().clone();
        unsafe {
            AppendMenuW(menu, MF_STRING, RESTORE, PCWSTR(labels.0.as_ptr()))?;
            AppendMenuW(menu, MF_STRING, QUIT, PCWSTR(labels.1.as_ptr()))?;
        }
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }?;
        let _ = unsafe { SetForegroundWindow(window) };
        let chosen = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                window,
                None,
            )
        }
        .0 as usize;
        let _ = unsafe { PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0)) };
        match chosen {
            RESTORE => (state.event)(Event::Restore),
            QUIT => (state.event)(Event::Quit),
            _ => {}
        }
        Ok(())
    })();
    unsafe { DestroyMenu(menu) }?;
    result
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    if message == WM_NCDESTROY {
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const State;
    if !pointer.is_null() {
        let state = unsafe { &*pointer };
        let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if message == state.taskbar_created {
                if !add(window, state) {
                    (state.event)(Event::Unavailable);
                }
                true
            } else if message == CALLBACK {
                match lparam.0 as u32 & 0xffff {
                    NIN_SELECT | NIN_KEYSELECT => (state.event)(Event::Restore),
                    WM_CONTEXTMENU => {
                        if menu(window, state).is_err() {
                            (state.event)(Event::Unavailable);
                        }
                    }
                    _ => {}
                }
                true
            } else {
                false
            }
        }));
        if matches!(handled, Ok(true)) {
            return LRESULT(0);
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

impl Tray {
    pub fn new(
        restore: &str,
        quit: &str,
        event: impl Fn(Event) + 'static,
    ) -> windows::core::Result<Self> {
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
        let class = w!("zabor-native-beta-tray");
        let definition = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&definition) } == 0
            && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(windows::core::Error::from_thread());
        }
        let icon = create_icon()?;
        let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        if taskbar_created == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let state = Box::new(State {
            icon,
            ready: Cell::new(false),
            taskbar_created,
            labels: RefCell::new((wide(restore), wide(quit))),
            event: Box::new(event),
        });
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!("zabor"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                Some((&*state as *const State).cast()),
            )
        }?;
        let tray = Self {
            window,
            state,
            thread_bound: PhantomData,
        };
        add(window, &tray.state);
        Ok(tray)
    }

    pub fn ready(&self) -> bool {
        self.state.ready.get()
    }

    pub fn labels(&self, restore: &str, quit: &str) {
        *self.state.labels.borrow_mut() = (wide(restore), wide(quit));
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.state.ready.set(false);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data(self.window, self.state.icon));
            SetWindowLongPtrW(self.window, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.window);
            let _ = DestroyIcon(self.state.icon);
        }
    }
}
