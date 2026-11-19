#![windows_subsystem = "windows"]

use windows_sys::Win32::UI::HiDpi::*;

mod engine;
mod input;
mod overlay;
mod settings;
mod startup;
mod ui;
use glidequill::{ACTIONS, DIRECTIONS};
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
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
enum Page {
    General = 130,
    Rules = 134,
    Feedback = 135,
}
impl Page {
    const ALL: [Self; 3] = [Self::General, Self::Rules, Self::Feedback];
    fn title(self) -> &'static str {
        match self {
            Self::General => "常规设置",
            Self::Rules => "手势规则",
            Self::Feedback => "轨迹与字幕",
        }
    }
}
const AUTOSTART: i32 = 131;
const IDLE: i32 = 132;
const LINGER: i32 = 133;
const IDLE_OPTION: i32 = 200;
const RESULT_OPTION: i32 = 210;
const CLASS: &str = "GlideQuill.Rules.0.1";
thread_local! {
    static TEST_INSTANCE:Cell<bool>=const{Cell::new(false)};
    static CONFIG_READABLE:Cell<bool>=const{Cell::new(true)};
    static PAGE:Cell<Page>=const{Cell::new(Page::Rules)};
    static STARTUP_ON:Cell<bool>=const{Cell::new(false)};
    static DARK:Cell<bool>=const{Cell::new(false)};
    static WINDOW:Cell<HWND>=const{Cell::new(null_mut())};
    static LISTENER:RefCell<Option<engine::Listener>>=const{RefCell::new(None)};
    static TRAY_ICON:Cell<HICON>=const{Cell::new(null_mut())};
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
fn time_label(id: i32, ms: u32) -> &'static str {
    match ms {
        0 if id == IDLE => "不取消",
        0 => "立即消失",
        250 => "0.25 秒",
        500 => "0.5 秒",
        550 => "0.55 秒",
        1000 => "1 秒",
        2000 => "2 秒",
        3000 => "3 秒",
        _ => unreachable!(),
    }
}
fn update_timing_labels() {
    let prefs = settings::preferences();
    label(
        IDLE,
        &format!("停笔后取消手势 · {}", time_label(IDLE, prefs.idle_ms)),
    );
    label(
        LINGER,
        &format!("松键后保留时间 · {}", time_label(LINGER, prefs.result_ms)),
    );
}
fn set_enabled(enabled: bool, persist: bool) {
    if !CONFIG_READABLE.with(Cell::get) {
        return;
    }
    let listener = if enabled {
        match engine::Listener::start(app_window(), TEST_INSTANCE.with(Cell::get)) {
            Ok(listener) => Some(listener),
            Err(e) => {
                label(RESULT, &format!("无法启动手势监听：{e}"));
                return;
            }
        }
    } else {
        None
    };
    if !enabled {
        LISTENER.with(|v| drop(v.borrow_mut().take()));
        overlay::reset();
        label(TOGGLE, "启用全局手势");
    }
    if persist
        && let Err(e) = settings::save(settings::Preferences {
            enabled,
            ..settings::preferences()
        })
    {
        label(RESULT, &format!("已暂停，启停状态保存失败：{e}"));
        return;
    }
    LISTENER.with(|v| *v.borrow_mut() = listener);
    label(
        TOGGLE,
        if enabled {
            "暂停全局手势"
        } else {
            "启用全局手势"
        },
    );
    label(
        RESULT,
        if enabled {
            "已启用 · 按住 Shift 可临时使用原生右键"
        } else {
            "已暂停 · 开启全局手势即可显示划动轨迹"
        },
    );
}
fn toggle() {
    set_enabled(LISTENER.with(|v| v.borrow().is_none()), true);
}
fn apply_theme(w: HWND, dark: bool) {
    DARK.with(|v| v.set(dark));
    unsafe {
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
}
fn refresh_startup() -> std::io::Result<bool> {
    let on = startup::registered()?;
    STARTUP_ON.with(|v| v.set(on));
    label(
        AUTOSTART,
        if on {
            "关闭登录自启"
        } else {
            "启用登录自启"
        },
    );
    Ok(on)
}
fn edit_time(id: i32) {
    let prefs = settings::preferences();
    let (choices, base, current) = if id == IDLE {
        (&settings::IDLE_VALUES, IDLE_OPTION, prefs.idle_ms)
    } else {
        (&settings::RESULT_VALUES, RESULT_OPTION, prefs.result_ms)
    };
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        for (i, &ms) in choices.iter().enumerate() {
            AppendMenuW(
                menu,
                MF_STRING | if ms == current { MF_CHECKED } else { 0 },
                (base + i as i32) as usize,
                wide(time_label(id, ms)).as_ptr(),
            );
        }
        let mut r = RECT::default();
        GetWindowRect(GetDlgItem(app_window(), id), &mut r);
        TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_TOPALIGN,
            r.left,
            r.bottom,
            0,
            app_window(),
            null_mut(),
        );
        DestroyMenu(menu);
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
            hIcon: TRAY_ICON.with(Cell::get),
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
                ui::layout();
                if overlay::create().is_err() {
                    return -1;
                }
                TASKBAR.with(|id| id.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr())));
                TRAY_OK.with(|ok| ok.set(notify(NIM_ADD)));
                0
            }
            WM_ACTIVATE
                if (wp & 0xffff) != WA_INACTIVE as usize
                    && PAGE.with(Cell::get) == Page::General =>
            {
                if let Err(e) = refresh_startup() {
                    label(RESULT, &format!("无法读取自启设置：{e}"));
                }
                DefWindowProcW(w, m, wp, lp)
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
                if let Some(page) = Page::ALL.into_iter().find(|&p| p as i32 == id) {
                    PAGE.with(|v| v.set(page));
                    if page == Page::General
                        && let Err(e) = refresh_startup()
                    {
                        label(RESULT, &format!("无法读取自启设置：{e}"));
                    }
                    ui::layout();
                    return 0;
                }
                match id {
                    id if (ui::PICKER..ui::PICKER + DIRECTIONS.len() as i32).contains(&id)
                        && wp >> 16 == CBN_SELCHANGE as usize =>
                    {
                        let field = GetDlgItem(w, id);
                        let index = (id - ui::PICKER) as usize;
                        let mut prefs = settings::preferences();
                        let previous = prefs.bindings[index];
                        let selected = SendMessageW(field, CB_GETCURSEL, 0, 0);
                        if CONFIG_READABLE.with(Cell::get)
                            && let Some(&action) = ACTIONS.get(selected as usize)
                        {
                            prefs.bindings[index] = action;
                            match settings::save(prefs) {
                                Ok(()) => label(
                                    RESULT,
                                    &format!(
                                        "已保存 · {}划 → {}",
                                        DIRECTIONS[index].name(),
                                        action.label()
                                    ),
                                ),
                                Err(e) => {
                                    SendMessageW(field, CB_SETCURSEL, previous as usize, 0);
                                    label(RESULT, &format!("绑定保存失败：{e}"));
                                }
                            }
                        } else {
                            SendMessageW(field, CB_SETCURSEL, previous as usize, 0);
                        }
                        InvalidateRect(field, null_mut(), 0);
                    }
                    AUTOSTART => {
                        match refresh_startup()
                            .and_then(|on| startup::set(!on))
                            .and_then(|()| refresh_startup())
                        {
                            Ok(on) => label(
                                RESULT,
                                if on {
                                    "已开启登录自启"
                                } else {
                                    "已关闭登录自启"
                                },
                            ),
                            Err(e) => label(RESULT, &format!("自启设置失败：{e}")),
                        }
                    }
                    THEME if CONFIG_READABLE.with(Cell::get) => {
                        let prefs = settings::preferences();
                        let dark = !prefs.dark;
                        match settings::save(settings::Preferences { dark, ..prefs }) {
                            Ok(()) => apply_theme(w, dark),
                            Err(e) => label(RESULT, &format!("外观保存失败：{e}")),
                        }
                    }
                    IDLE | LINGER if CONFIG_READABLE.with(Cell::get) => edit_time(id),
                    id if CONFIG_READABLE.with(Cell::get)
                        && ((IDLE_OPTION..IDLE_OPTION + 5).contains(&id)
                            || (RESULT_OPTION..RESULT_OPTION + 5).contains(&id)) =>
                    {
                        let mut prefs = settings::preferences();
                        if id >= RESULT_OPTION {
                            prefs.result_ms =
                                settings::RESULT_VALUES[(id - RESULT_OPTION) as usize];
                        } else {
                            prefs.idle_ms = settings::IDLE_VALUES[(id - IDLE_OPTION) as usize];
                        }
                        match settings::save(prefs) {
                            Ok(()) => {
                                update_timing_labels();
                                label(RESULT, "轨迹时间已保存");
                            }
                            Err(e) => label(RESULT, &format!("设置保存失败：{e}")),
                        }
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
        AdjustWindowRectExForDpi(&mut frame, WINDOW_STYLE, 0, WS_EX_DLGMODALFRAME, dpi);
    }
    frame
}
fn main() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let background = std::env::args_os().skip(1).any(|a| a == "--startup");
        let test_id = std::env::var("GLIDEQUILL_UI_TEST_ID")
            .ok()
            .filter(|id| id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        TEST_INSTANCE.with(|v| v.set(test_id.is_some()));
        if let Some(id) = &test_id {
            startup::isolate_test(id);
        }
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
            if background {
                return;
            }
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
        TRAY_ICON.with(|v| v.set(icon));
        let wc = WNDCLASSW {
            hInstance: inst,

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
            WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            wide("设置").as_ptr(),
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
        update_timing_labels();
        let prefs = settings::preferences();
        apply_theme(hwnd, prefs.dark);
        if let Some(ref e) = error {
            label(RESULT, &format!("配置读取失败，已暂停：{e}"));
            for id in [TOGGLE, IDLE, LINGER, THEME]
                .into_iter()
                .chain(ui::PICKER..ui::PICKER + DIRECTIONS.len() as i32)
            {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                    GetDlgItem(hwnd, id),
                    0,
                );
            }
        } else if prefs.enabled {
            set_enabled(true, false);
        }
        if !background
            || error.is_some()
            || !TRAY_OK.with(Cell::get)
            || (prefs.enabled && LISTENER.with(|v| v.borrow().is_none()))
        {
            ShowWindow(hwnd, SW_SHOW);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
