use crate::{overlay, settings};
use glidequill::{Action, Stroke};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK;

use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    ptr::null_mut,
};
use windows_sys::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
use windows_sys::Win32::{Graphics::Gdi::*, System::Threading::*, UI::Input::KeyboardAndMouse::*};

struct Target {
    window: HWND,
    foreground: HWND,
    pid: u32,
    rules: settings::Resolved,
    browser: bool,
}
#[derive(Clone, Copy)]
struct PendingShortcut {
    target: HWND,
    pid: u32,
    requested: u32,
    shortcut: Shortcut,
}
const ACTIVATION_TIMEOUT_MS: u32 = 350;
const WINDOW_ACTION_DELAY_MS: u32 = 180;

#[derive(Clone, Copy)]
struct ReleasedWindowAction {
    window: HWND,
    foreground: HWND,
    pid: u32,
    action: Action,
    released_at: u32,
    timer: usize,
}

struct Capture {
    target: Target,
    started: u32,
    cancelled: bool,
    origin: POINT,
    dpi: u32,
    stroke: Stroke,
    points: glidequill::library::Path,
    last_shape: Option<Action>,
    last_match: Option<std::time::Instant>,
    wheel_delta: i32,
    used_wheel: bool,
}
impl Capture {
    fn sample(&mut self, point: POINT) {
        let scale = 96.0 / self.dpi as f64;
        let p = [
            ((point.x - self.origin.x) as f64 * scale).round() as i32,
            ((point.y - self.origin.y) as f64 * scale).round() as i32,
        ];
        let factor = 24. / f64::from(self.target.rules.tuning.distance);
        self.stroke.sample(
            (p[0] as f64 * factor).round() as i32,
            (p[1] as f64 * factor).round() as i32,
        );
        if !self.target.rules.shapes.is_empty() {
            if self.points.is_empty() {
                self.points.push([0, 0]);
            }
            glidequill::library::append_sample(&mut self.points, p);
        }
    }
    fn action(&mut self, final_sample: bool) -> Option<Action> {
        let rules = &self.target.rules;
        if !rules.shapes.is_empty()
            && self.stroke.long_enough()
            && (final_sample
                || self
                    .last_match
                    .is_none_or(|t| t.elapsed().as_millis() >= 24))
        {
            self.last_match = Some(std::time::Instant::now());
            self.last_shape = glidequill::library::signature(&self.points).and_then(|s| {
                use glidequill::library::Match;
                match glidequill::library::closest(
                    &s,
                    &rules.shapes,
                    f64::from(rules.tuning.tolerance) / 100.,
                ) {
                    Match::Hit(action) => Some(*action),
                    Match::Ambiguous => Some(Action::Unassigned),
                    Match::Miss => None,
                }
            });
        }
        self.last_shape
            .or_else(|| self.stroke.gesture().map(|d| rules.bindings[d as usize]))
    }
}
thread_local! {
    static CAPTURE:RefCell<Option<Capture>>=const{RefCell::new(None)};
    static RELEASED: Cell<Option<ReleasedWindowAction>> = const { Cell::new(None) };
    static PENDING: Cell<Option<PendingShortcut>> = const { Cell::new(None) };
    static SETTINGS_WINDOW:Cell<HWND>=const{Cell::new(null_mut())};
    static PASS_THROUGH:Cell<bool>=const{Cell::new(false)};
}
pub fn initialize(window: HWND, pass_through: bool) {
    SETTINGS_WINDOW.with(|w| w.set(window));
    PASS_THROUGH.with(|v| v.set(pass_through));
}
unsafe fn eligible(point: POINT) -> Option<Target> {
    unsafe {
        let target = GetAncestor(WindowFromPhysicalPoint(point), GA_ROOT);
        let foreground = GetForegroundWindow();
        if foreground.is_null()
            || target.is_null()
            || target == GetDesktopWindow()
            || target == GetShellWindow()
            || target == crate::library_ui::recording_window()
            || IsWindowEnabled(target) == 0
            || GetWindowLongW(target, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE != 0
        {
            return None;
        }
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut rect = RECT::default();
        if settings::preferences().pause_fullscreen
            && GetMonitorInfoW(MonitorFromWindow(target, MONITOR_DEFAULTTONEAREST), &mut mi) != 0
            && GetWindowRect(target, &mut rect) != 0
            && rect.left <= mi.rcMonitor.left
            && rect.top <= mi.rcMonitor.top
            && rect.right >= mi.rcMonitor.right
            && rect.bottom >= mi.rcMonitor.bottom
            // A maximized application can cover the monitor on an auto-hidden taskbar.
            && !(IsZoomed(target) != 0 && GetWindowLongW(target, GWL_STYLE) as u32 & (WS_CAPTION | WS_MAXIMIZEBOX) != 0)
        {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(target, &mut pid);
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(proc, 0, buf.as_mut_ptr(), &mut n);
        let elevated = is_elevated(proc);
        CloseHandle(proc);
        if ok == 0 || elevated {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..n as usize]);
        let exe = path.rsplit('\\').next()?;
        (!settings::excluded(exe)).then(|| Target {
            window: target,
            foreground,
            pid,
            rules: settings::resolve(exe),
            browser: exe.eq_ignore_ascii_case("chrome.exe")
                || exe.eq_ignore_ascii_case("msedge.exe"),
        })
    }
}
unsafe fn is_elevated(proc: HANDLE) -> bool {
    unsafe {
        use windows_sys::Win32::Security::*;
        let mut token = null_mut();
        if OpenProcessToken(proc, TOKEN_QUERY, &mut token) == 0 {
            return true;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok == 0 || elevation.TokenIsElevated != 0
    }
}
// A switch away cancels this stroke even if the original window regains focus before release.
pub unsafe extern "system" fn foreground_changed(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    _: i32,
    _: i32,
    _: u32,
    event_time: u32,
) {
    if let Some(released) = RELEASED.get()
        && hwnd != released.foreground
        && event_time.wrapping_sub(released.released_at) as i32 >= 0
    {
        cancel_window_action();
    }
    // Foreground activation across input queues can complete after mouse-up.
    if let Some(pending) = PENDING.get()
        && event_time.wrapping_sub(pending.requested) as i32 >= 0
    {
        PENDING.set(None);
        if hwnd == pending.target
            && unsafe { GetTickCount() }.wrapping_sub(pending.requested) <= ACTIVATION_TIMEOUT_MS
        {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == pending.pid {
                    send_shortcut(hwnd, pending.shortcut);
                }
            }
        }
    }
    CAPTURE.with_borrow_mut(|capture| {
        if let Some(s) = capture
            && s.used_wheel
            && !s.cancelled
            && hwnd == s.target.window
            && event_time.wrapping_sub(s.started) as i32 >= 0
        {
            s.target.foreground = hwnd;
        }
        if let Some(s) = capture
            && !s.cancelled
            && hwnd != s.target.foreground
            && event_time.wrapping_sub(s.started) as i32 >= 0
        {
            s.cancelled = true;
            overlay::cancel(SETTINGS_WINDOW.get());
        }
    });
}

fn modifiers_pressed() -> bool {
    [
        VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, VK_LBUTTON, VK_MBUTTON,
    ]
    .iter()
    .any(|&key| unsafe { GetAsyncKeyState(key as i32) < 0 })
}

fn cancel_window_action() {
    if let Some(pending) = RELEASED.take() {
        unsafe {
            KillTimer(null_mut(), pending.timer);
        }
    }
}

unsafe fn schedule_window_action(target: &Target, action: Action) {
    cancel_window_action();
    unsafe {
        let timer = SetTimer(
            null_mut(),
            0,
            WINDOW_ACTION_DELAY_MS,
            Some(commit_window_action),
        );
        if timer != 0 {
            RELEASED.set(Some(ReleasedWindowAction {
                window: target.window,
                foreground: target.foreground,
                pid: target.pid,
                action,
                released_at: GetTickCount(),
                timer,
            }));
        }
    }
}

unsafe extern "system" fn commit_window_action(_: HWND, _: u32, timer: usize, _: u32) {
    let Some(pending) = RELEASED.get().filter(|pending| pending.timer == timer) else {
        return;
    };
    unsafe {
        let elapsed = GetTickCount().wrapping_sub(pending.released_at);
        if elapsed < WINDOW_ACTION_DELAY_MS {
            return;
        }
        cancel_window_action();
        let mut pid = 0;
        GetWindowThreadProcessId(pending.window, &mut pid);
        if elapsed < 1000
            && pid == pending.pid
            && GetForegroundWindow() == pending.foreground
            && IsWindowEnabled(pending.window) != 0
            && IsIconic(pending.window) == 0
            && GetAsyncKeyState(VK_RBUTTON.into()) >= 0
        {
            execute(pending.window, pending.action, false);
        }
    }
}

// Windows owns minimize/maximize placement even when an app draws its own caption.
unsafe fn change_window_state(target: HWND, action: Action) {
    unsafe {
        let style = GetWindowLongW(target, GWL_STYLE) as u32;
        let (show, command) = match action {
            Action::Minimize if style & WS_MINIMIZEBOX != 0 => (SW_MINIMIZE, SC_MINIMIZE),
            Action::Maximize if IsZoomed(target) != 0 => (SW_RESTORE, SC_RESTORE),
            Action::Maximize if style & WS_MAXIMIZEBOX != 0 => (SW_MAXIMIZE, SC_MAXIMIZE),
            _ => return,
        };
        if style & WS_SYSMENU != 0 {
            // Chromium updates its native menu on demand. Its cached Restore item
            // can still be grey on a maximized window. Let the browser handle the
            // same system command as its own caption buttons.
            let mut class = [0; 64];
            let len = GetClassNameW(target, class.as_mut_ptr(), class.len() as i32);
            if class[..len as usize]
                .iter()
                .copied()
                .eq("Chrome_WidgetWin_1".encode_utf16())
            {
                PostMessageW(target, WM_SYSCOMMAND, command as usize, 0);
                return;
            }
            let menu = GetSystemMenu(target, 0);
            if !menu.is_null() {
                let state = GetMenuState(menu, command, MF_BYCOMMAND);
                if state != u32::MAX && state & (MF_DISABLED | MF_GRAYED) != 0 {
                    return;
                }
            }
        }
        ShowWindowAsync(target, show);
    }
}

unsafe fn close_window(target: HWND) {
    unsafe {
        let style = GetWindowLongW(target, GWL_STYLE) as u32;
        if style & WS_SYSMENU == 0 {
            // A custom-caption application may implement its own normal WM_CLOSE flow.
            if style & (WS_MINIMIZEBOX | WS_MAXIMIZEBOX) != 0 {
                PostMessageW(target, WM_CLOSE, 0, 0);
            }
        } else {
            let menu = GetSystemMenu(target, 0);
            if !menu.is_null()
                && GetMenuState(menu, SC_CLOSE, MF_BYCOMMAND) & (MF_GRAYED | MF_DISABLED) == 0
            {
                PostMessageW(target, WM_SYSCOMMAND, SC_CLOSE as usize, 0);
            }
        }
    }
}

fn key_event(key: u16, flags: u32) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(key.into(), MAPVK_VK_TO_VSC_EX) };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: (scan & 0xff) as u16,
                dwFlags: flags
                    | if scan >> 8 == 0xe0 {
                        KEYEVENTF_EXTENDEDKEY
                    } else {
                        0
                    },
                ..Default::default()
            },
        },
    }
}
// Submit one complete shortcut to the active application's normal input queue.
#[derive(Clone, Copy)]
enum Shortcut {
    Keys(glidequill::Chord),
    BrowserCommand(u32),
}
fn shortcut(action: Action, browser: bool) -> Option<Shortcut> {
    use windows_sys::Win32::System::SystemServices::*;
    if browser {
        let command = match action {
            Action::NewTab => Some(APPCOMMAND_NEW),
            Action::CloseTab => Some(APPCOMMAND_CLOSE),
            Action::BrowserHome => Some(APPCOMMAND_BROWSER_HOME),
            Action::Refresh => Some(APPCOMMAND_BROWSER_REFRESH),
            _ => None,
        };
        if let Some(c) = command {
            return Some(Shortcut::BrowserCommand(c));
        }
    }
    action.keys().map(Shortcut::Keys)
}
unsafe fn send_shortcut(target: HWND, shortcut: Shortcut) {
    unsafe {
        if GetForegroundWindow() != target || modifiers_pressed() {
            return;
        }
        let chord = match shortcut {
            Shortcut::Keys(c) => c,
            Shortcut::BrowserCommand(c) => {
                PostMessageW(target, WM_APPCOMMAND, target as usize, (c << 16) as isize);
                return;
            }
        };
        let keys: Vec<_> = chord
            .modifiers()
            .chain(std::iter::once(chord.key()))
            .collect();
        if keys.iter().any(|&key| GetAsyncKeyState(key.into()) < 0)
            || modifiers_pressed()
            || GetForegroundWindow() != target
        {
            return;
        }
        let events: Vec<_> = keys
            .iter()
            .map(|&key| key_event(key, 0))
            .chain(
                keys.iter()
                    .rev()
                    .map(|&key| key_event(key, KEYEVENTF_KEYUP)),
            )
            .collect();
        let size = size_of::<INPUT>() as i32;
        let sent = SendInput(events.len() as u32, events.as_ptr(), size) as usize;
        let held = sent.min(events.len().saturating_sub(sent));
        if held > 0 {
            let ups = &events[events.len() - held..];
            SendInput(ups.len() as u32, ups.as_ptr(), size);
        }
    }
}
unsafe fn execute(target: HWND, action: Action, browser: bool) {
    unsafe {
        #[cfg(test)]
        {
            let expected = browser_tests::TEST_TARGET.load(std::sync::atomic::Ordering::SeqCst);
            if !expected.is_null() && expected != target {
                return;
            }
        }
        if modifiers_pressed() {
            return;
        }
        let Some(shortcut) = shortcut(action, browser) else {
            match action {
                Action::Minimize | Action::Maximize => change_window_state(target, action),
                Action::Close => close_window(target),
                _ => {}
            }
            return;
        };
        if GetForegroundWindow() == target {
            send_shortcut(target, shortcut);
            return;
        }

        let mut pid = 0;
        GetWindowThreadProcessId(target, &mut pid);
        PENDING.set(Some(PendingShortcut {
            target,
            pid,
            requested: GetTickCount(),
            shortcut,
        }));
        if SetForegroundWindow(target) == 0 {
            PENDING.set(None);
        } else if GetForegroundWindow() == target && PENDING.take().is_some() {
            send_shortcut(target, shortcut);
        }
        // Otherwise the foreground notification completes or cancels the shortcut.
    }
}
pub unsafe extern "system" fn hook(code: i32, msg: WPARAM, data: LPARAM) -> LRESULT {
    unsafe {
        if code < 0 || PASS_THROUGH.with(Cell::get) {
            return CallNextHookEx(null_mut(), code, msg, data);
        }
        let e = &*(data as *const MSLLHOOKSTRUCT);
        #[cfg(test)]
        let ignore = e.dwExtraInfo != browser_tests::TEST_MOUSE_TAG;
        #[cfg(not(test))]
        let ignore = e.flags & LLMHF_INJECTED != 0;
        if ignore {
            return CallNextHookEx(null_mut(), code, msg, data);
        }
        if msg as u32 == WM_MOUSEWHEEL && wheel((e.mouseData >> 16) as i16 as i32) {
            return 1;
        }
        if gesture(msg as u32, e.pt) {
            1
        } else {
            CallNextHookEx(null_mut(), code, msg, data)
        }
    }
}
// Partial high-resolution deltas accumulate until a whole detent. Once the
// wheel owns the press, releasing the button must not replay a context click.
unsafe fn wheel(delta: i32) -> bool {
    if delta == 0 {
        return false;
    }
    let mut dispatch = None;
    let consumed = CAPTURE.with_borrow_mut(|capture| {
        let Some(s) = capture else {
            return false;
        };
        if s.cancelled
            || !s.stroke.is_click()
            || modifiers_pressed()
            || unsafe { GetForegroundWindow() } != s.target.foreground
        {
            return false;
        }
        let Some(action) = s.target.rules.wheel[usize::from(delta < 0)] else {
            return false;
        };
        if s.wheel_delta.signum() != delta.signum() {
            s.wheel_delta = 0;
        }
        s.used_wheel = true;
        s.wheel_delta += delta;
        let steps = s.wheel_delta.unsigned_abs() / 120;
        s.wheel_delta %= 120;
        if steps > 0 {
            dispatch = Some((s.target.window, action, steps, s.target.browser));
        }
        true
    });
    if let Some((window, action, steps, browser)) = dispatch {
        for _ in 0..steps.min(8) {
            unsafe {
                execute(window, action, browser);
            }
        }
    }
    consumed
}
unsafe fn gesture(msg: u32, point: POINT) -> bool {
    unsafe {
        if matches!(
            msg,
            WM_RBUTTONDOWN
                | WM_LBUTTONDOWN
                | WM_MBUTTONDOWN
                | WM_XBUTTONDOWN
                | WM_MOUSEWHEEL
                | WM_MOUSEHWHEEL
        ) {
            PENDING.set(None);
            cancel_window_action();
        }
        match msg {
            WM_RBUTTONDOWN => {
                if modifiers_pressed() || GetSystemMetrics(SM_SWAPBUTTON) != 0 {
                    false
                } else if let Some(target) = eligible(point) {
                    let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(target.window).max(96);
                    overlay::begin(
                        SETTINGS_WINDOW.with(Cell::get),
                        target.foreground,
                        point,
                        dpi,
                    );
                    CAPTURE.with(|c| {
                        *c.borrow_mut() = Some(Capture {
                            target,
                            started: GetTickCount(),
                            cancelled: false,
                            origin: point,
                            dpi,
                            stroke: Stroke::new(0, 0),
                            points: Vec::new(),
                            last_match: None,
                            wheel_delta: 0,
                            used_wheel: false,
                            last_shape: None,
                        })
                    });
                    true
                } else {
                    false
                }
            }
            WM_MOUSEMOVE => {
                CAPTURE.with(|c| {
                    if let Some(s) = c.borrow_mut().as_mut() {
                        if s.cancelled || s.used_wheel {
                            return;
                        }
                        s.sample(point);
                        if s.target.foreground != GetForegroundWindow() || modifiers_pressed() {
                            s.cancelled = true;
                            overlay::cancel(SETTINGS_WINDOW.with(Cell::get));
                        } else if !s.stroke.is_click() {
                            overlay::extend(
                                SETTINGS_WINDOW.with(Cell::get),
                                point,
                                s.action(false),
                            );
                        }
                    }
                });
                false
            }
            WM_RBUTTONUP => {
                if let Some(mut s) = CAPTURE.with(|c| c.borrow_mut().take()) {
                    s.sample(point);
                    let window = SETTINGS_WINDOW.with(Cell::get);
                    let mut pid = 0;
                    GetWindowThreadProcessId(s.target.window, &mut pid);
                    if s.cancelled
                        || s.used_wheel
                        || pid != s.target.pid
                        || IsWindowEnabled(s.target.window) == 0
                        || IsIconic(s.target.window) != 0
                        || s.target.foreground != GetForegroundWindow()
                        || modifiers_pressed()
                    {
                        overlay::cancel(window);
                    } else if s.stroke.is_click() {
                        overlay::cancel(window);
                        if GetAncestor(WindowFromPhysicalPoint(point), GA_ROOT) == s.target.window {
                            replay_click();
                        }
                    } else {
                        let action = s.action(true);
                        if let Some(action) = overlay::finish(window, point, action) {
                            if matches!(action, Action::Minimize | Action::Maximize | Action::Close)
                            {
                                schedule_window_action(&s.target, action);
                            } else {
                                execute(s.target.window, action, s.target.browser);
                            }
                        }
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
unsafe fn replay_click() {
    unsafe {
        let batch = [MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP].map(|flag| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: flag,
                    ..Default::default()
                },
            },
        });
        SendInput(2, batch.as_ptr(), size_of::<INPUT>() as i32);
    }
}
#[cfg(test)]
static DESKTOP_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ui, wide};
    fn received(window: HWND, message: u32) -> Vec<MSG> {
        let mut messages = Vec::new();
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, window, message, message, PM_REMOVE) != 0 {
                messages.push(msg);
            }
        }
        messages
    }
    fn keys(window: HWND, count: usize) -> Vec<(u16, bool)> {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(120);
        let mut result = Vec::new();
        loop {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, window, WM_KEYFIRST, WM_KEYLAST, PM_REMOVE) } != 0
            {
                match msg.message {
                    WM_KEYDOWN | WM_SYSKEYDOWN => result.push((msg.wParam as u16, true)),
                    WM_KEYUP | WM_SYSKEYUP => result.push((msg.wParam as u16, false)),
                    _ => {}
                }
            }
            if (count > 0 && result.len() >= count) || std::time::Instant::now() >= until {
                return result;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    // Runs on an interactive desktop. Feed physical-coordinate samples through the
    // production gesture handler without moving the user's pointer or injecting clicks.
    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn native_trail_shows_before_release_and_preserves_the_foreground() {
        let _desktop = DESKTOP_TEST.lock().unwrap();
        unsafe {
            use windows_sys::Win32::Graphics::Dwm::DwmFlush;
            use windows_sys::Win32::UI::HiDpi::*;
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _graphics = ui::GraphicsSession::start();
            let class = wide("GlideQuill.SmokeSurface");
            let instance = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(null_mut());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                lpszClassName: class.as_ptr(),
                hInstance: instance,
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&wc), 0);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                wide("GlideQuill overlay test").as_ptr(),
                WS_POPUP | WS_VISIBLE,
                80,
                80,
                480,
                360,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            assert!(!hwnd.is_null());
            SetForegroundWindow(hwnd);
            assert_eq!(GetForegroundWindow(), hwnd);
            crate::WINDOW.with(|v| v.set(hwnd));
            initialize(hwnd, false);
            overlay::create().unwrap();
            let popup = overlay::test_popup();
            assert!(!popup.is_null());
            UpdateWindow(hwnd);
            DwmFlush();
            let subtitle = overlay::test_subtitle();
            let start = POINT { x: 280, y: 240 };
            // Load user bindings so the real HUD must follow the chosen action for each diagonal.
            let test_dir =
                std::env::temp_dir().join(format!("glidequill-bindings-{}", std::process::id()));
            std::fs::create_dir(&test_dir).unwrap();
            let name = std::env::current_exe()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let mut overrides = [None; glidequill::GESTURES.len()];
            overrides[4] = Some(glidequill::Action::Copy);
            overrides[7] = Some(glidequill::Action::Unassigned);
            let config = serde_json::json!({
                "bindings": ["back", "forward", "home", "end", "home", "paste", "minimize", "end",
                    "end", "unassigned", "unassigned", "unassigned", "unassigned", "copy", "unassigned", "unassigned"],
                "applications": { (name.to_lowercase()): overrides }
            });
            std::fs::write(
                test_dir.join("settings.json"),
                serde_json::to_vec(&config).unwrap(),
            )
            .unwrap();
            settings::load_from(&test_dir).unwrap();
            assert_eq!(
                settings::resolve("another.exe").bindings[4],
                glidequill::Action::Home
            );
            assert_eq!(
                settings::resolve(&name.to_uppercase()).bindings[4],
                glidequill::Action::Copy
            );
            settings::save_exclusions_in(&test_dir, &[name]).unwrap();
            assert_eq!(
                eligible(start).map(|target| target.window),
                None,
                "saved exclusion must bypass the next stroke"
            );
            assert!(!gesture(WM_RBUTTONDOWN, start));
            assert!(!gesture(WM_RBUTTONUP, start));
            settings::save_exclusions_in(&test_dir, &[]).unwrap();
            assert_eq!(eligible(start).map(|target| target.window), Some(hwnd));
            std::fs::remove_file(test_dir.join("excluded.json")).unwrap();
            std::fs::remove_file(test_dir.join("settings.json")).unwrap();
            std::fs::remove_dir(test_dir).unwrap();
            // Exercise the ordinary-click replay inside the disposable test window.
            let mut original_pointer = POINT::default();
            assert_ne!(GetCursorPos(&mut original_pointer), 0);
            assert_ne!(SetCursorPos(start.x, start.y), 0);
            let click_consumed = gesture(WM_RBUTTONDOWN, start) && gesture(WM_RBUTTONUP, start);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
            let (mut down, mut up) = (0, 0);
            while std::time::Instant::now() < until && up == 0 {
                down += received(hwnd, WM_RBUTTONDOWN).len();
                up += received(hwnd, WM_RBUTTONUP).len();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            SetCursorPos(original_pointer.x, original_pointer.y);
            assert!(click_consumed);
            assert_eq!((down, up), (1, 1));
            for (x, y, expected) in [
                (180, 240, "后退"),
                (380, 240, "前进"),
                (280, 140, "页面顶部"),
                (280, 340, "页面底部"),
                (190, 150, "复制"),
                (370, 150, "粘贴"),
                (190, 330, "最小化窗口"),
                (370, 330, ""),
            ] {
                let end = POINT { x, y };
                assert_eq!(
                    eligible(start).map(|target| target.window),
                    Some(hwnd),
                    "hit={:?} foreground={:?}",
                    WindowFromPoint(start),
                    GetForegroundWindow()
                );
                assert!(gesture(WM_RBUTTONDOWN, start));
                overlay::refresh(hwnd);
                assert_eq!(IsWindowVisible(popup), 0);
                assert!(!gesture(WM_MOUSEMOVE, end));
                overlay::refresh(hwnd);
                assert_ne!(IsWindowVisible(popup), 0);
                DwmFlush();
                let screen = GetDC(null_mut());
                let color = GetPixel(screen, (x + start.x) / 2, (y + start.y) / 2);
                ReleaseDC(null_mut(), screen);
                assert_eq!(color, 0xd36838, "live stroke at screen midpoint");
                let matched = !expected.is_empty();
                assert_eq!(IsWindowVisible(subtitle) != 0, matched);
                if matched {
                    let mut buf = [0u16; 32];
                    let n = GetWindowTextW(subtitle, buf.as_mut_ptr(), 32);
                    assert_eq!(String::from_utf16_lossy(&buf[..n as usize]), expected);
                    let mut rect = RECT::default();
                    GetWindowRect(subtitle, &mut rect);
                    let mut m = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    assert_ne!(
                        GetMonitorInfoW(MonitorFromPoint(end, MONITOR_DEFAULTTONEAREST), &mut m),
                        0
                    );
                    let work = m.rcWork;
                    assert!((rect.left + rect.right - work.left - work.right).abs() <= 1);
                    assert!(rect.top > (work.top + work.bottom) / 2 && rect.bottom <= work.bottom);
                }
                assert_eq!(GetForegroundWindow(), hwnd);
                assert_eq!(GetAncestor(WindowFromPoint(end), GA_ROOT), hwnd);
                assert!(received(hwnd, WM_APPCOMMAND).is_empty());
                assert!(received(hwnd, WM_KEYDOWN).is_empty());
                assert!(gesture(WM_RBUTTONUP, end));
                assert!(!gesture(WM_RBUTTONUP, end));
                let chord = |key| {
                    vec![
                        (VK_CONTROL, true),
                        (key, true),
                        (key, false),
                        (VK_CONTROL, false),
                    ]
                };
                let pair = |key| vec![(key, true), (key, false)];
                let expected_keys = match expected {
                    "后退" => pair(VK_BROWSER_BACK),
                    "前进" => pair(VK_BROWSER_FORWARD),
                    "复制" => chord(VK_C),
                    "粘贴" => chord(VK_V),
                    "页面顶部" => pair(VK_HOME),
                    "页面底部" => pair(VK_END),
                    _ => Vec::new(),
                };
                assert_eq!(keys(hwnd, expected_keys.len()), expected_keys);
                overlay::refresh(hwnd);
                let mut rect = RECT::default();
                GetWindowRect(popup, &mut rect);
                assert_eq!(IsWindowVisible(subtitle) != 0, matched);
                if matched {
                    assert!(
                        rect.right <= start.x.max(end.x) + 16,
                        "stroke surface grew to subtitle"
                    );
                }
                assert_eq!(GetForegroundWindow(), hwnd);
            }
            // Continuous rounded U and L strokes, with an offset and shorter return.
            for (path, expected) in [
                (
                    [
                        (284, 215),
                        (290, 188),
                        (300, 160),
                        (312, 148),
                        (323, 150),
                        (331, 166),
                        (338, 186),
                        (344, 206),
                    ],
                    "页面底部",
                ),
                (
                    [
                        (278, 211),
                        (275, 184),
                        (279, 161),
                        (287, 148),
                        (300, 141),
                        (321, 136),
                        (345, 131),
                        (370, 128),
                    ],
                    "复制",
                ),
            ] {
                assert!(gesture(WM_RBUTTONDOWN, start));
                for (x, y) in path {
                    gesture(WM_MOUSEMOVE, POINT { x, y });
                }
                let (x, y) = path[path.len() - 1];
                let end = POINT { x, y };
                overlay::refresh(hwnd);
                assert_ne!(IsWindowVisible(subtitle), 0);
                let mut buf = [0; 32];
                let n = GetWindowTextW(subtitle, buf.as_mut_ptr(), 32);
                assert_eq!(String::from_utf16_lossy(&buf[..n as usize]), expected);
                assert!(gesture(WM_RBUTTONUP, end));
                overlay::refresh(hwnd);
                assert_ne!(IsWindowVisible(subtitle), 0);
                let n = GetWindowTextW(subtitle, buf.as_mut_ptr(), 32);
                assert_eq!(String::from_utf16_lossy(&buf[..n as usize]), expected);
            }
            std::thread::sleep(std::time::Duration::from_millis(600));
            overlay::refresh(hwnd);
            assert_eq!(IsWindowVisible(popup), 0);
            assert_eq!(IsWindowVisible(subtitle), 0);
            // A diagonal entering a bend must have intermediate edge pixels,
            // not a staircase made entirely of solid ink and background.
            assert!(gesture(WM_RBUTTONDOWN, start));
            for (x, y) in [(315, 221), (336, 224), (361, 237), (382, 245)] {
                gesture(WM_MOUSEMOVE, POINT { x, y });
            }
            overlay::refresh(hwnd);
            DwmFlush();
            let screen = GetDC(null_mut());
            let mut blended = false;
            for y in 224..237 {
                for x in 294..303 {
                    let c = GetPixel(screen, x, y);
                    let (r, g, b) = (c & 255, (c >> 8) & 255, (c >> 16) & 255);
                    blended |= r > 56 && r < 245 && g > r && b > g;
                }
            }
            ReleaseDC(null_mut(), screen);
            assert!(blended, "antialiased curve edge");
            assert!(gesture(WM_RBUTTONUP, POINT { x: 382, y: 245 }));
            keys(hwnd, 0);
            // Even after returning to the starting window, a cancelled capture stays cancelled.
            assert!(gesture(WM_RBUTTONDOWN, start));
            foreground_changed(
                null_mut(),
                EVENT_SYSTEM_FOREGROUND,
                GetDesktopWindow(),
                0,
                0,
                0,
                GetTickCount(),
            );
            let across = POINT {
                x: start.x + 100,
                y: start.y,
            };
            gesture(WM_MOUSEMOVE, across);
            assert!(gesture(WM_RBUTTONUP, across));
            assert!(keys(hwnd, 0).is_empty());
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let requested = GetTickCount();
            PENDING.set(Some(PendingShortcut {
                target: hwnd,
                pid,
                requested,
                shortcut: Shortcut::Keys("Home".parse().unwrap()),
            }));
            for activated in [GetDesktopWindow(), hwnd] {
                foreground_changed(
                    null_mut(),
                    EVENT_SYSTEM_FOREGROUND,
                    activated,
                    0,
                    0,
                    0,
                    requested,
                );
            }
            assert!(PENDING.get().is_none());
            assert!(
                keys(hwnd, 0).is_empty(),
                "cancelled activation must not send keys"
            );
            // The held stroke expires independently of mouse-up, and cannot revive.
            let end = POINT { x: 380, y: 240 };
            assert!(gesture(WM_RBUTTONDOWN, start));
            gesture(WM_MOUSEMOVE, end);
            overlay::refresh(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(600));
            overlay::refresh(hwnd);
            assert_eq!(IsWindowVisible(popup), 0);
            gesture(WM_MOUSEMOVE, POINT { x: 400, y: 240 });
            assert!(gesture(WM_RBUTTONUP, end));
            overlay::refresh(hwnd);
            assert_eq!(IsWindowVisible(popup), 0);

            assert!(keys(hwnd, 0).is_empty());

            // With idle cancellation disabled the same pause keeps the trail. Zero
            // release time must hide it on the first frame after release.
            let dir =
                std::env::temp_dir().join(format!("glidequill-timing-{}", std::process::id()));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("settings.json"), r#"{"idle_ms":0,"result_ms":0}"#).unwrap();
            settings::load_from(&dir).unwrap();
            std::fs::remove_file(dir.join("settings.json")).unwrap();

            assert!(gesture(WM_RBUTTONDOWN, start));
            gesture(WM_MOUSEMOVE, end);
            overlay::refresh(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(600));
            overlay::refresh(hwnd);
            assert_ne!(IsWindowVisible(popup), 0);
            assert!(gesture(WM_RBUTTONUP, end));
            overlay::refresh(hwnd);
            assert_eq!(IsWindowVisible(popup), 0);
            assert_eq!(
                keys(hwnd, 2),
                [(VK_BROWSER_FORWARD, true), (VK_BROWSER_FORWARD, false)]
            );
            let mut monitor = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            assert_ne!(
                GetMonitorInfoW(
                    MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                    &mut monitor
                ),
                0
            );
            let r = monitor.rcMonitor;
            SetWindowPos(
                hwnd,
                HWND_TOP,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_SHOWWINDOW,
            );
            assert_eq!(
                eligible(start).map(|target| target.window),
                None,
                "fullscreen must be bypassed by default"
            );
            std::fs::write(dir.join("settings.json"), r#"{"pause_fullscreen":false}"#).unwrap();
            settings::load_from(&dir).unwrap();
            assert_eq!(
                eligible(start).map(|target| target.window),
                Some(hwnd),
                "fullscreen opt-in must take effect"
            );
            std::fs::remove_file(dir.join("settings.json")).unwrap();
            std::fs::remove_dir(dir).unwrap();
            // Consume edit messages without altering the interactive user's clipboard.
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                null_mut(),
                WS_CHILD | WS_VISIBLE,
                10,
                10,
                120,
                35,
                hwnd,
                null_mut(),
                instance,
                null_mut(),
            );
            assert!(!edit.is_null());
            SetFocus(edit);
            for (action, key) in [(Action::Copy, VK_C), (Action::Paste, VK_V)] {
                execute(hwnd, action, false);
                assert_eq!(
                    keys(edit, 4),
                    [
                        (VK_CONTROL, true),
                        (key, true),
                        (key, false),
                        (VK_CONTROL, false)
                    ]
                );
                assert!(received(hwnd, WM_APPCOMMAND).is_empty());
            }
            DestroyWindow(edit);
            let sample = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("GlideQuill window action test").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                100,
                100,
                400,
                300,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            );
            assert!(!sample.is_null());
            SetForegroundWindow(sample);
            let menu = GetSystemMenu(sample, 0);
            EnableMenuItem(menu, SC_MINIMIZE, MF_BYCOMMAND | MF_GRAYED);
            execute(sample, Action::Minimize, false);
            let mut msg = MSG::default();
            PeekMessageW(&mut msg, sample, 0, 0, PM_NOREMOVE);
            assert_eq!(IsIconic(sample), 0, "explicitly disabled minimize");
            execute(sample, Action::Close, false);
            let messages = received(sample, WM_SYSCOMMAND);
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].wParam, SC_CLOSE as usize);
            DispatchMessageW(&messages[0]);
            assert_eq!(IsWindow(sample), 0);
            overlay::destroy();
            DestroyWindow(hwnd);
        }
    }
}

#[cfg(test)]
mod browser_tests;
