#![windows_subsystem = "windows"]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, ReleaseCapture, SetCapture};

mod input;
mod ui;
use glidequill::{ACTIONS, Action, Config, DIRECTIONS, Stroke};
use std::{
    cell::{Cell, RefCell},
    io,
    mem::size_of,
    path::PathBuf,
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    Storage::FileSystem::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Shell::*, WindowsAndMessaging::*},
};
const RUN: u32 = WM_APP + 1;
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
    static DARK:Cell<bool>=const{Cell::new(false)};
    static TRAIL:RefCell<Vec<POINT>>=const{RefCell::new(Vec::new())};
    static CONFIG:RefCell<Config>=RefCell::new(Config::default());
    static WINDOW:Cell<HWND>=const{Cell::new(null_mut())};
    static HOOK:Cell<HHOOK>=const{Cell::new(null_mut())};
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
fn path() -> io::Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(io::ErrorKind::NotFound)?)
            .join("GlideQuill/rules.json"),
    )
}
fn load() -> io::Result<Config> {
    match std::fs::read(path()?) {
        Ok(b) => Ok(serde_json::from_slice(&b)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e),
    }
}
fn save(c: &Config) -> io::Result<()> {
    let file = path()?;
    std::fs::create_dir_all(file.parent().unwrap())?;
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(c)?)?;
    if unsafe {
        MoveFileExW(
            wide(&tmp.to_string_lossy()).as_ptr(),
            wide(&file.to_string_lossy()).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn label(id: i32, s: &str) {
    unsafe {
        SetWindowTextW(GetDlgItem(app_window(), id), wide(s).as_ptr());
    }
}
unsafe fn control(class: &str, s: &str, id: i32, style: u32, area: [i32; 4]) -> HWND {
    unsafe {
        let [x, y, w, h] = area;
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
    CONFIG.with(|c| {
        let c = c.borrow();
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
    });
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
fn toggle() {
    if TEST_INSTANCE.with(Cell::get) {
        return;
    }
    unsafe {
        let old = HOOK.with(Cell::get);
        if old.is_null() {
            let h = SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(input::hook),
                GetModuleHandleW(null_mut()),
                0,
            );
            if h.is_null() {
                label(RESULT, "无法启动手势监听");
                return;
            }
            HOOK.with(|v| v.set(h));
            label(TOGGLE, "暂停全局手势");
            label(RESULT, "已启用 · Shift + 右键临时放行");
        } else {
            UnhookWindowsHookEx(old);
            HOOK.with(|v| v.set(null_mut()));
            input::cancel();
            label(TOGGLE, "启用全局手势");
            label(RESULT, "已暂停 · 可以继续在窗口内试画");
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
            hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
            ..Default::default()
        };
        let tip = wide("GlideQuill · 拂标");
        n.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(op, &n) != 0
    }
}
unsafe extern "system" fn pad_proc(w: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let x = i32::from(lp as i16);
        let y = i32::from((lp >> 16) as i16);
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
                        let mut c = CONFIG.with(|c| c.borrow().clone());
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
                SetBkMode(dc, TRANSPARENT as i32);
                let dark = DARK.with(Cell::get);
                ui::pad_background(dc, r, TRAIL.with(|t| t.borrow().len() < 2));
                TRAIL.with(|trail| {
                    let trail = trail.borrow();
                    if trail.len() > 1 {
                        let pen =
                            CreatePen(PS_SOLID, 3, if dark { 0x00FFCCA4 } else { 0x00A25324 });
                        let old = SelectObject(dc, pen);
                        Polyline(dc, trail.as_ptr(), trail.len() as i32);
                        SelectObject(dc, old);
                        DeleteObject(pen);
                    }
                });
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
                ui::build();
                populate();
                TASKBAR.with(|id| id.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr())));
                TRAY_OK.with(|ok| ok.set(notify(NIM_ADD)));
                0
            }
            WM_PAINT => {
                ui::paint(w);
                0
            }
            WM_ERASEBKGND => 1,
            WM_CTLCOLORSTATIC => ui::static_style(wp as HDC),
            WM_MEASUREITEM => {
                let item = &mut *(lp as *mut windows_sys::Win32::UI::Controls::MEASUREITEMSTRUCT);
                item.itemHeight = 24;
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
                        let mut draft = CONFIG.with(|c| c.borrow().clone());
                        read_rules(&mut draft);
                        if CONFIG.with(|c| {
                            draft.global != c.borrow().global || draft.browser != c.borrow().browser
                        }) && MessageBoxW(
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
                        label(
                            RESULT,
                            if id == BROWSER {
                                "正在编辑浏览器覆盖规则"
                            } else {
                                "正在编辑通用规则"
                            },
                        );
                    }
                    SAVE => {
                        let mut c = CONFIG.with(|c| c.borrow().clone());
                        read_rules(&mut c);
                        match save(&c) {
                            Ok(()) => {
                                CONFIG.with(|v| *v.borrow_mut() = c);
                                label(RESULT, "已保存 · 当前规则立即生效");
                            }
                            Err(e) => label(RESULT, &format!("保存失败：{e}")),
                        }
                    }
                    THEME => {
                        let dark = DARK.with(|d| {
                            d.set(!d.get());
                            d.get()
                        });
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
            RUN => {
                if !HOOK.with(Cell::get).is_null() && wp as HWND == GetForegroundWindow() {
                    let dir = DIRECTIONS[(lp & 3) as usize];
                    input::perform(
                        wp as HWND,
                        CONFIG.with(|c| c.borrow().resolve(lp & 256 != 0, dir)),
                    );
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
                            if HOOK.with(Cell::get).is_null() {
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
                if !HOOK.with(Cell::get).is_null() {
                    toggle();
                }
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
fn main() {
    unsafe {
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
        let cfg = load();
        let error = cfg.as_ref().err().map(ToString::to_string);
        if let Ok(c) = cfg {
            CONFIG.with(|v| *v.borrow_mut() = c)
        }
        let inst = GetModuleHandleW(null_mut());
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
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: ui::WIDTH,
            bottom: ui::HEIGHT,
        };
        AdjustWindowRectEx(
            &mut frame,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            0,
            0,
        );
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("GlideQuill · 手势规则").as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
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
