#![windows_subsystem = "windows"]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, ReleaseCapture, SetCapture};

use windows_sys::Win32::UI::HiDpi::*;

mod engine;
mod input;
mod settings;
mod ui;
use glidequill::{ACTIONS, Action, Config, Stroke};
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
const SCOPE: i32 = 100;
const FIRST: i32 = 110;
const SAVE: i32 = 120;
const TOGGLE: i32 = 121;
const RESULT: i32 = 122;
const OPEN: i32 = 123;
const EXIT: i32 = 124;
const THEME: i32 = 127;
const PAD: i32 = 125;
const BROWSER: i32 = 126;
const CLASS: &str = "GlideQuill.Rules.0.1";
thread_local! {
    static TEST_INSTANCE:Cell<bool>=const{Cell::new(false)};
    static CONFIG_READABLE:Cell<bool>=const{Cell::new(true)};
    static DARK:Cell<bool>=const{Cell::new(false)};
    static TRAIL:RefCell<Vec<POINT>>=const{RefCell::new(Vec::new())};
    static WINDOW:Cell<HWND>=const{Cell::new(null_mut())};
    static LISTENER:RefCell<Option<engine::Listener>>=const{RefCell::new(None)};
    static GROUP:Cell<bool>=const{Cell::new(false)};
    static PRACTICE:RefCell<Option<Stroke>>=const{RefCell::new(None)};
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
fn populate() {
    let group = GROUP.with(Cell::get);
    {
        let c = settings::snapshot();
        let rules = if group { c.browser } else { c.global };
        for (i, a) in rules.iter().enumerate() {
            unsafe {
                let cb = GetDlgItem(app_window(), FIRST + i as i32);
                SendMessageW(cb, CB_RESETCONTENT, 0, 0);
                for (action, name) in ACTIONS
                    .iter()
                    .filter(|(a, _)| group || *a != Action::Inherit)
                {
                    let n = SendMessageW(cb, CB_ADDSTRING, 0, wide(name).as_ptr() as isize);
                    if action == a {
                        SendMessageW(cb, CB_SETCURSEL, n as usize, 0);
                    }
                }
            }
        }
    }
}
fn read_rules(c: &mut Config) {
    let group = GROUP.with(Cell::get);
    let rules = if group { &mut c.browser } else { &mut c.global };
    for (i, a) in rules.iter_mut().enumerate() {
        let ix = unsafe {
            SendMessageW(
                GetDlgItem(app_window(), FIRST + i as i32),
                CB_GETCURSEL,
                0,
                0,
            )
        };
        if ix >= 0 {
            *a = ACTIONS[ix as usize + usize::from(!group)].0;
        }
    }
}
fn dirty_rules() -> bool {
    let active = settings::snapshot();
    let mut draft = (*active).clone();
    read_rules(&mut draft);
    draft.global != active.global || draft.browser != active.browser
}
fn update_save() {
    unsafe {
        EnableWindow(
            GetDlgItem(app_window(), SAVE),
            (CONFIG_READABLE.with(Cell::get) && dirty_rules()).into(),
        );
    }
}
fn toggle() {
    let active = LISTENER.with(|v| v.borrow_mut().take());
    if let Some(listener) = active {
        drop(listener);
        label(TOGGLE, "启用全局手势");
        label(RESULT, "已暂停 · 可以继续在窗口内试画");
    } else {
        match engine::Listener::start(app_window(), TEST_INSTANCE.with(Cell::get)) {
            Ok(listener) => {
                LISTENER.with(|v| *v.borrow_mut() = Some(listener));
                label(TOGGLE, "暂停全局手势");
                label(RESULT, "已启用 · Shift + 右键临时放行");
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
unsafe extern "system" fn pad_proc(w: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let x = ui::dip(i32::from(lp as i16));
        let y = ui::dip(i32::from((lp >> 16) as i16));
        match m {
            WM_RBUTTONDOWN => {
                SetCapture(w);
                TRAIL.with(|p| {
                    let mut p = p.borrow_mut();
                    p.clear();
                    p.push(POINT { x, y });
                });
                InvalidateRect(w, null_mut(), 0);
                PRACTICE.with(|p| *p.borrow_mut() = Some(Stroke::new(x, y)));
                0
            }
            WM_MOUSEMOVE => {
                PRACTICE.with(|p| {
                    if let Some(s) = p.borrow_mut().as_mut() {
                        s.sample(x, y);
                        TRAIL.with(|p| {
                            let mut p = p.borrow_mut();
                            if p.len() < 2048 {
                                p.push(POINT { x, y });
                            }
                        });
                        InvalidateRect(w, null_mut(), 0);
                    }
                });
                0
            }
            WM_RBUTTONUP => {
                let stroke = PRACTICE.with(|p| p.borrow_mut().take());
                ReleaseCapture();
                if let Some(mut s) = stroke {
                    s.sample(x, y);
                    if let Some(dir) = s.direction() {
                        let mut c = (*settings::snapshot()).clone();
                        read_rules(&mut c);
                        let a = c.resolve(GROUP.with(Cell::get), dir);
                        label(
                            RESULT,
                            &format!(
                                "试画命中：{} → {}",
                                ["向左", "向右", "向上", "向下"][dir as usize],
                                ACTIONS.iter().find(|v| v.0 == a).unwrap().1
                            ),
                        );
                    } else {
                        label(RESULT, "未识别 · 请画一条较长的直线")
                    }
                }
                0
            }
            WM_CAPTURECHANGED => {
                PRACTICE.with(|p| p.borrow_mut().take());
                0
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(w, &mut ps);
                let mut r = RECT::default();
                GetClientRect(w, &mut r);
                TRAIL.with(|t| ui::paint_pad(dc, r, &t.borrow()));
                EndPaint(w, &ps);
                0
            }
            _ => DefWindowProcW(w, m, wp, lp),
        }
    }
}
unsafe extern "system" fn proc(w: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_CREATE => {
                WINDOW.with(|v| v.set(w));
                ui::set_dpi(GetDpiForWindow(w));
                ui::build();
                populate();
                update_save();
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
            WM_MEASUREITEM => {
                let item = &mut *(lp as *mut windows_sys::Win32::UI::Controls::MEASUREITEMSTRUCT);
                item.itemHeight = ui::px(26) as u32;
                1
            }
            WM_DRAWITEM => {
                ui::item(&*(lp as *const windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT));
                1
            }
            WM_COMMAND => {
                let id = (wp & 0xffff) as i32;
                match id {
                    SCOPE | BROWSER => {
                        if (id == BROWSER) == GROUP.with(Cell::get) {
                            return 0;
                        }
                        if dirty_rules()
                            && MessageBoxW(
                                w,
                                wide("切换范围将放弃未保存的修改，继续？").as_ptr(),
                                wide("拂标").as_ptr(),
                                MB_OKCANCEL,
                            ) != IDOK
                        {
                            return 0;
                        }
                        GROUP.with(|v| v.set(id == BROWSER));
                        RedrawWindow(w, null_mut(), null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
                        populate();
                        update_save();
                        label(
                            RESULT,
                            if id == BROWSER {
                                "正在编辑浏览器覆盖规则"
                            } else {
                                "正在编辑通用规则"
                            },
                        );
                    }
                    SAVE if CONFIG_READABLE.with(Cell::get) && dirty_rules() => {
                        let mut c = (*settings::snapshot()).clone();
                        read_rules(&mut c);
                        match settings::apply(c) {
                            Ok(()) => {
                                update_save();
                                label(RESULT, "已保存 · 当前规则立即生效");
                            }
                            Err(e) => label(RESULT, &format!("保存失败：{e}")),
                        }
                    }
                    id if (FIRST..FIRST + 4).contains(&id)
                        && (wp >> 16) == CBN_SELCHANGE as usize =>
                    {
                        update_save()
                    }
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
        let pad = wide("GlideQuill.Practice");
        for (name, handler, brush) in [
            (
                &class,
                Some(proc as unsafe extern "system" fn(_, _, _, _) -> _),
                COLOR_WINDOW,
            ),
            (&pad, Some(pad_proc), COLOR_BTNFACE),
        ] {
            let wc = WNDCLASSW {
                hInstance: inst,
                hIcon: icon,
                lpszClassName: name.as_ptr(),
                lpfnWndProc: handler,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: GetSysColorBrush(brush),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return;
            }
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
            EnableWindow(GetDlgItem(hwnd, SAVE), 0);
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
