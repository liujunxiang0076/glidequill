#![windows_subsystem = "windows"]

use windows_sys::Win32::UI::HiDpi::*;

mod engine;
mod input;
mod overlay;
mod settings;
mod ui;
use glidequill::DIRECTIONS;
use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Shell::*, WindowsAndMessaging::*},
};
const TRAY: u32 = WM_APP + 2;
const TOGGLE: i32 = 121;
const RESULT: i32 = 122;
const OPEN: i32 = 123;
const EXIT: i32 = 124;
const THEME: i32 = 127;
const CLASS: &str = "GlideQuill.Rules.0.1";
thread_local! {
    static TEST_INSTANCE:Cell<bool>=const{Cell::new(false)};
    static CONFIG_READABLE:Cell<bool>=const{Cell::new(true)};
    static DARK:Cell<bool>=const{Cell::new(false)};
    static WINDOW:Cell<HWND>=const{Cell::new(null_mut())};
    static LISTENER:RefCell<Option<engine::Listener>>=const{RefCell::new(None)};
    static TRAY_OK:Cell<bool>=const{Cell::new(false)};
    static TASKBAR:Cell<u32>=const{Cell::new(0)};
}
fn app_window() -> HWND {
    WINDOW.with(Cell::get)
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn label(id: i32, s: &str) {
    unsafe {
        let child = GetDlgItem(app_window(), id);
        SetWindowTextW(child, wide(s).as_ptr());
        InvalidateRect(child, null_mut(), 0);
    }
}
unsafe fn control(class: &str, s: &str, id: i32, style: u32, area: [i32; 4]) -> HWND {
    unsafe {
        let [x, y, w, h] = area.map(ui::px);
        let hwnd = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(s).as_ptr(),
            style | WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            app_window(),
            id as HMENU,
            GetModuleHandleW(null_mut()),
            null_mut(),
        );
        SendMessageW(
            hwnd,
            WM_SETFONT,
            GetStockObject(DEFAULT_GUI_FONT) as usize,
            1,
        );
        hwnd
    }
}
fn toggle() {
    if !CONFIG_READABLE.with(Cell::get) {
        return;
    }
    let active = LISTENER.with(|v| v.borrow_mut().take());
    if let Some(listener) = active {
        drop(listener);
        overlay::reset();
        label(TOGGLE, "启用全局手势");
        label(RESULT, "已暂停 · 开启全局手势即可显示划动轨迹");
    } else {
        match engine::Listener::start(app_window(), TEST_INSTANCE.with(Cell::get)) {
            Ok(listener) => {
                LISTENER.with(|v| *v.borrow_mut() = Some(listener));
                label(TOGGLE, "暂停全局手势");
                label(RESULT, "已启用 · 按住 Shift 可临时使用原生右键");
            }
            Err(e) => label(RESULT, &format!("无法启动手势监听：{e}")),
        }
    }
}
unsafe fn notify(op: u32) -> bool {
    unsafe {
        let mut n = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: app_window(),
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: GetClassLongPtrW(app_window(), GCLP_HICON) as HICON,
            ..Default::default()
        };
        let tip = wide("GlideQuill · 拂标");
        n.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(op, &n) != 0
    }
}
unsafe extern "system" fn proc(w: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_CREATE => {
                WINDOW.with(|v| v.set(w));
                ui::set_dpi(GetDpiForWindow(w));
                ui::build();
                if overlay::create().is_err() {
                    return -1;
                }
                TASKBAR.with(|id| id.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr())));
                TRAY_OK.with(|ok| ok.set(notify(NIM_ADD)));
                0
            }
            WM_PAINT => {
                ui::paint(w);
                0
            }
            WM_ERASEBKGND => 1,
            WM_DPICHANGED => {
                let dpi = (wp & 0xffff) as u32;
                ui::set_dpi(dpi);
                let suggested = &*(lp as *const RECT);
                let frame = frame_size(dpi);
                SetWindowPos(
                    w,
                    null_mut(),
                    suggested.left,
                    suggested.top,
                    frame.right - frame.left,
                    frame.bottom - frame.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                ui::layout();
                0
            }
            overlay::REFRESH => {
                overlay::refresh(w);
                0
            }
            WM_TIMER if wp == overlay::TIMER => {
                overlay::refresh(w);
                0
            }
            WM_DRAWITEM => {
                ui::item(&*(lp as *const windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT));
                1
            }
            WM_COMMAND => {
                let id = (wp & 0xffff) as i32;
                match id {
                    THEME => {
                        let dark = DARK.with(|d| {
                            d.set(!d.get());
                            d.get()
                        });
                        use windows_sys::Win32::Graphics::Dwm::*;
                        let use_dark = i32::from(dark);
                        DwmSetWindowAttribute(
                            w,
                            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                            (&use_dark as *const i32).cast(),
                            size_of::<i32>() as u32,
                        );
                        label(THEME, if dark { "浅色" } else { "深色" });
                        RedrawWindow(
                            w,
                            null_mut(),
                            null_mut(),
                            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
                        );
                    }
                    TOGGLE => toggle(),
                    OPEN => {
                        ShowWindow(w, SW_RESTORE);
                        SetForegroundWindow(w);
                    }
                    EXIT => {
                        DestroyWindow(w);
                    }
                    _ => {}
                }
                0
            }
            TRAY => {
                if lp as u32 == WM_LBUTTONDBLCLK {
                    PostMessageW(w, WM_COMMAND, OPEN as usize, 0);
                } else if lp as u32 == WM_RBUTTONUP {
                    let menu = CreatePopupMenu();
                    for (id, name) in [
                        (OPEN, "打开规则窗口"),
                        (
                            TOGGLE,
                            if LISTENER.with(|v| v.borrow().is_none()) {
                                "启用手势"
                            } else {
                                "暂停手势"
                            },
                        ),
                        (EXIT, "退出"),
                    ] {
                        AppendMenuW(menu, MF_STRING, id as usize, wide(name).as_ptr());
                    }
                    let mut pt = POINT::default();
                    GetCursorPos(&mut pt);
                    SetForegroundWindow(w);
                    TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, w, null_mut());
                    DestroyMenu(menu);
                }
                0
            }
            WM_CLOSE => {
                if TRAY_OK.with(Cell::get) {
                    ShowWindow(w, SW_HIDE);
                } else {
                    DestroyWindow(w);
                }
                0
            }
            WM_DESTROY => {
                LISTENER.with(|v| drop(v.borrow_mut().take()));
                overlay::destroy();
                notify(NIM_DELETE);
                PostQuitMessage(0);
                0
            }
            _ if m != 0 && m == TASKBAR.with(Cell::get) => {
                TRAY_OK.with(|v| v.set(notify(NIM_ADD)));
                0
            }
            _ => DefWindowProcW(w, m, wp, lp),
        }
    }
}
const WINDOW_STYLE: u32 =
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
unsafe fn frame_size(dpi: u32) -> RECT {
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: ui::px(ui::WIDTH),
        bottom: ui::px(ui::HEIGHT),
    };
    unsafe {
        AdjustWindowRectExForDpi(&mut frame, WINDOW_STYLE, 0, 0, dpi);
    }
    frame
}
fn main() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let test_id = std::env::var("GLIDEQUILL_UI_TEST_ID")
            .ok()
            .filter(|id| id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        TEST_INSTANCE.with(|v| v.set(test_id.is_some()));
        let suffix = test_id.map(|id| format!(".test.{id}")).unwrap_or_default();
        let class = wide(&format!("{CLASS}{suffix}"));
        // Keep the handle live for the message loop, including startup and shutdown.
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        let mutex = windows_sys::Win32::System::Threading::CreateMutexW(
            null_mut(),
            0,
            wide(&format!(r"Local\GlideQuill.Rules.Instance{suffix}")).as_ptr(),
        );
        if mutex.is_null() {
            return;
        }
        let exists = GetLastError() == ERROR_ALREADY_EXISTS;
        let _instance = OwnedHandle::from_raw_handle(mutex);
        if exists {
            for _ in 0..40 {
                let h = FindWindowW(class.as_ptr(), null_mut());
                if !h.is_null() {
                    PostMessageW(h, WM_COMMAND, OPEN as usize, 0);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            return;
        }
        let cfg = settings::load();
        let error = cfg.as_ref().err().map(ToString::to_string);
        CONFIG_READABLE.with(|v| v.set(error.is_none()));
        if let Ok(c) = cfg {
            settings::initialize(c)
        }
        let _graphics = ui::GraphicsSession::start();
        let inst = GetModuleHandleW(null_mut());
        let image = include_bytes!("quill.png");
        let icon = CreateIconFromResourceEx(
            image.as_ptr(),
            image.len() as u32,
            1,
            0x30000,
            96,
            96,
            LR_DEFAULTCOLOR,
        );
        let wc = WNDCLASSW {
            hInstance: inst,
            hIcon: icon,
            lpszClassName: class.as_ptr(),
            lpfnWndProc: Some(proc),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        let dpi = GetDpiForSystem();
        ui::set_dpi(dpi);
        let frame = frame_size(dpi);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("GlideQuill · 手势规则").as_ptr(),
            WINDOW_STYLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            frame.right - frame.left,
            frame.bottom - frame.top,
            null_mut(),
            null_mut(),
            inst,
            null_mut(),
        );
        if hwnd.is_null() {
            return;
        }
        let actual_dpi = GetDpiForWindow(hwnd);
        if actual_dpi != dpi {
            let actual = frame_size(actual_dpi);
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                actual.right - actual.left,
                actual.bottom - actual.top,
                SWP_NOZORDER | SWP_NOMOVE | SWP_NOACTIVATE,
            );
        }
        if let Some(e) = error {
            label(RESULT, &format!("配置读取失败，已暂停：{e}"));
            windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                GetDlgItem(hwnd, TOGGLE),
                0,
            );
        }
        ShowWindow(hwnd, SW_SHOW);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
