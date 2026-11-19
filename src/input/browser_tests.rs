//! Interactive regression: inspect the result in a separate, disposable Chromium process.
use super::*;
use std::{
    path::PathBuf,
    process::{Child, Command},
    time::{Duration, Instant},
};

fn title(window: HWND) -> String {
    let mut buf = [0; 256];
    let n = unsafe { GetWindowTextW(window, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n as usize])
}
// Keep the same layered windows visible during dispatch as on the interactive desktop.
pub(super) static TEST_TARGET: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
pub(super) const TEST_MOUSE_TAG: usize = 0x4751_5453;
struct FeedbackWindow {
    host: HWND,
    listener: Option<crate::engine::Listener>,
    previous_cursor: POINT,
}
impl FeedbackWindow {
    fn new() -> Self {
        unsafe {
            let host = CreateWindowExW(
                0,
                crate::wide("STATIC").as_ptr(),
                null_mut(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            assert!(!host.is_null());
            crate::WINDOW.set(host);
            overlay::create().unwrap();
            let mut previous_cursor = POINT::default();
            assert_ne!(GetCursorPos(&mut previous_cursor), 0);
            let listener = crate::engine::Listener::start(host, false).unwrap();
            Self {
                host,
                listener: Some(listener),
                previous_cursor,
            }
        }
    }
}
impl Drop for FeedbackWindow {
    fn drop(&mut self) {
        unsafe {
            drop(self.listener.take());
            TEST_TARGET.store(null_mut(), std::sync::atomic::Ordering::SeqCst);
            SetCursorPos(self.previous_cursor.x, self.previous_cursor.y);
            overlay::destroy();
            DestroyWindow(self.host);
        }
        crate::WINDOW.set(null_mut());
    }
}
struct Browser {
    child: Child,
    dir: PathBuf,
    window: HWND,
}
impl Browser {
    fn start(profile: &str) -> Self {
        let chrome = std::env::var_os("GLIDEQUILL_TEST_BROWSER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("ProgramFiles").unwrap())
                    .join("Google/Chrome/Application/chrome.exe")
            });
        let dir = std::env::temp_dir().join(format!(
            "glidequill-browser-{profile}-{}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let page = dir.join("test.html");
        std::fs::write(
            &page,
            r##"<!doctype html><meta charset=utf-8><title>GlideQuill action test</title>
<style>body{height:350vh}a{position:fixed;inset:20%;display:block}</style><a autofocus href="#detail">Open detail</a>
<script>
function report() {
  const locationName = location.hash ? 'detail' : 'index';
  const position = scrollY < 1 ? 'top' : (innerHeight + scrollY >= document.documentElement.scrollHeight - 2 ? 'end' : 'middle');
  document.title = 'GlideQuill action test ' + locationName + '-' + position;
}
onhashchange = onscrollend = report;
onload = () => requestAnimationFrame(() => requestAnimationFrame(() => { document.querySelector("a").focus(); report(); }));
let editKey = '';
onkeydown = e => { if (e.ctrlKey) editKey = e.code; };
for (const type of ['copy','paste']) document.addEventListener(type, event => {
  event.preventDefault();
  document.title = 'GlideQuill action test ' + type + '-' + editKey; editKey = '';
});
</script>"##,
        )
        .unwrap();
        let child = Command::new(chrome)
            .arg(format!("--user-data-dir={}", dir.join("profile").display()))
            .args([
                "--new-window",
                "--disable-sync",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-mode",
                "--disable-smooth-scrolling",
                "--window-size=740,560",
            ])
            .arg(format!(
                "file:///{}",
                page.display()
                    .to_string()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            ))
            .spawn()
            .expect("set GLIDEQUILL_TEST_BROWSER to a Chromium executable");
        let mut browser = Self {
            child,
            dir,
            window: null_mut(),
        };
        unsafe extern "system" fn find_test_page(window: HWND, data: LPARAM) -> i32 {
            let browser = unsafe { &mut *(data as *mut Browser) };
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(window, &mut pid);
            }
            if pid == browser.child.id() && title(window).contains("GlideQuill action test") {
                browser.window = window;
                return 0;
            }
            1
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            unsafe {
                EnumWindows(Some(find_test_page), (&mut browser as *mut Self) as isize);
                if !browser.window.is_null() {
                    SetForegroundWindow(browser.window);
                    if GetForegroundWindow() == browser.window {
                        return browser;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("test page {:?} did not become foreground", browser.window);
    }
    fn activate(&self) {
        // Restoring a minimized browser completes asynchronously on its UI thread.
        let start = Instant::now();
        unsafe {
            while GetForegroundWindow() != self.window && start.elapsed() < Duration::from_secs(2) {
                SetForegroundWindow(self.window);
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(GetForegroundWindow(), self.window);
        }
    }
    fn expect_page(&self, state: &str) {
        let wanted = format!("GlideQuill action test {state}");
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            // Pump the test settings window while the input listener runs independently.
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if title(self.window).contains(&wanted) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("expected {wanted}, observed {}", title(self.window));
    }
    #[track_caller]
    fn expect_test_tab(&self, visible: bool) {
        let start = Instant::now();
        loop {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            if title(self.window).contains("GlideQuill action test") == visible {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(3),
                "tab did not change: {} target={:?} foreground={:?}, CTRL={} T={}",
                title(self.window),
                self.window,
                unsafe { GetForegroundWindow() },
                unsafe { GetAsyncKeyState(17) },
                unsafe { GetAsyncKeyState(84) }
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    // Restoring a window animates its bounds. A floating utility may also
    // cover its center while the surrounding page is visible and interactive.
    unsafe fn exposed_point(&self) -> POINT {
        unsafe {
            let start = Instant::now();
            let mut previous = None;
            let mut stable_since = start;
            loop {
                let mut rect = RECT::default();
                assert_ne!(GetWindowRect(self.window, &mut rect), 0);
                let bounds = (rect.left, rect.top, rect.right, rect.bottom);
                if previous != Some(bounds) {
                    previous = Some(bounds);
                    stable_since = Instant::now();
                } else if stable_since.elapsed() >= Duration::from_millis(120) {
                    for y in [50, 35, 65] {
                        for x in [50, 35, 65] {
                            let point = POINT {
                                x: rect.left + (rect.right - rect.left) * x / 100,
                                y: rect.top + (rect.bottom - rect.top) * y / 100,
                            };
                            if GetAncestor(WindowFromPhysicalPoint(point), GA_ROOT) == self.window {
                                return point;
                            }
                        }
                    }
                }
                assert!(
                    start.elapsed() < Duration::from_secs(4),
                    "test surface not exposed: {bounds:?}"
                );
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }
    fn test_path(&self, offsets: &[[i32; 2]], wheel: Option<i32>) {
        self.activate();
        unsafe {
            let center = self.exposed_point();
            TEST_TARGET.store(self.window, std::sync::atomic::Ordering::SeqCst);
            mouse_event_at(center, MOUSEEVENTF_MOVE);
            mouse_event_at(center, MOUSEEVENTF_RIGHTDOWN);
            let mut end = center;
            for &[dx, dy] in offsets {
                end = POINT {
                    x: center.x + dx,
                    y: center.y + dy,
                };
                mouse_event_at(end, MOUSEEVENTF_MOVE);
                std::thread::sleep(Duration::from_millis(8));
            }
            if let Some(delta) = wheel {
                for _ in 0..2 {
                    let wheel = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                mouseData: (delta / 2) as u32,
                                dwFlags: MOUSEEVENTF_WHEEL,
                                dwExtraInfo: TEST_MOUSE_TAG,
                                ..Default::default()
                            },
                        },
                    };
                    assert_eq!(SendInput(1, &wheel, size_of::<INPUT>() as i32), 1);
                }
            }
            mouse_event_at(end, MOUSEEVENTF_RIGHTUP);
        }
    }
    fn swipe(&self, dx: i32, dy: i32) {
        self.activate();
        self.swipe_with_foreground(dx, dy, self.window);
    }
    fn swipe_with_foreground(&self, dx: i32, dy: i32, foreground: HWND) {
        unsafe {
            assert_eq!(GetForegroundWindow(), foreground);
            let center = self.exposed_point();
            let zoom = IsZoomed(self.window);
            TEST_TARGET.store(self.window, std::sync::atomic::Ordering::SeqCst);
            mouse_event_at(center, MOUSEEVENTF_MOVE);
            mouse_event_at(center, MOUSEEVENTF_RIGHTDOWN);
            struct HeldButton(POINT);
            impl Drop for HeldButton {
                fn drop(&mut self) {
                    unsafe { mouse_event_at(self.0, MOUSEEVENTF_RIGHTUP) };
                }
            }
            let mut held = HeldButton(center);
            for i in 1..=12 {
                held.0 = POINT {
                    x: center.x + dx * i / 12,
                    y: center.y + dy * i / 12,
                };
                mouse_event_at(held.0, MOUSEEVENTF_MOVE);
                if i == 6 {
                    std::thread::sleep(Duration::from_millis(200));
                    assert_ne!(IsWindow(self.window), 0);
                    assert_eq!(
                        (IsZoomed(self.window), IsIconic(self.window)),
                        (zoom, 0),
                        "window moved mid-stroke"
                    );
                }
                std::thread::sleep(Duration::from_millis(6));
            }
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(250) {
                overlay::refresh(crate::app_window());
                if IsWindowVisible(overlay::test_subtitle()) != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_ne!(
                IsWindowVisible(overlay::test_subtitle()),
                0,
                "expected a matched action caption"
            );
            assert_eq!(
                GetForegroundWindow(),
                foreground,
                "feedback changed focus before release"
            );
            drop(held);
            assert_eq!(
                (IsZoomed(self.window), IsIconic(self.window)),
                (zoom, 0),
                "window changed on release without settling"
            );
            overlay::refresh(crate::app_window());
        }
    }
}
// Exercise the OS hook queue at desktop coordinates, including the physical
// right-button state. Only the test binary accepts these tagged samples.
unsafe fn mouse_event_at(point: POINT, flags: u32) {
    unsafe {
        let normalized = |value, origin, extent| {
            (((value - origin) as i64 * 65536 + 32768) / extent as i64) as i32
        };
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: normalized(
                        point.x,
                        GetSystemMetrics(SM_XVIRTUALSCREEN),
                        GetSystemMetrics(SM_CXVIRTUALSCREEN),
                    ),
                    dy: normalized(
                        point.y,
                        GetSystemMetrics(SM_YVIRTUALSCREEN),
                        GetSystemMetrics(SM_CYVIRTUALSCREEN),
                    ),
                    dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    dwExtraInfo: TEST_MOUSE_TAG,
                    ..Default::default()
                },
            },
        };
        assert_eq!(SendInput(1, &input, size_of::<INPUT>() as i32), 1);
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        if !self.window.is_null() {
            unsafe {
                let mut pid = 0;
                GetWindowThreadProcessId(self.window, &mut pid);
                if pid == self.child.id() {
                    PostMessageW(self.window, WM_CLOSE, 0, 0);
                }
            }
        }
        let start = Instant::now();
        while self.child.try_wait().ok().flatten().is_none()
            && start.elapsed() < Duration::from_secs(3)
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
#[test]
#[ignore = "requires a foreground desktop and an installed Chromium browser"]
fn external_browser_actions() {
    let _desktop = DESKTOP_TEST.lock().unwrap();
    unsafe {
        use windows_sys::Win32::UI::HiDpi::*;
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let _graphics = crate::ui::GraphicsSession::start();
    let _feedback = FeedbackWindow::new();
    let browser = Browser::start("target");
    let mut bindings = glidequill::DEFAULT_BINDINGS;
    bindings[glidequill::Gesture::DownLeft as usize] = Action::Minimize;
    bindings[glidequill::Gesture::UpRight as usize] = Action::Maximize;
    bindings[glidequill::Gesture::UpLeft as usize] = Action::Copy;
    bindings[glidequill::Gesture::DownRight as usize] = Action::Paste;
    let config = serde_json::json!({"bindings": bindings});
    std::fs::write(
        browser.dir.join("settings.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    settings::load_from(&browser.dir).unwrap();
    unsafe {
        browser.expect_page("index-top");
        std::thread::sleep(Duration::from_millis(400));
        // Activate the local link to seed real browser history.
        assert_eq!(GetForegroundWindow(), browser.window);
        let link = browser.exposed_point();
        mouse_event_at(link, MOUSEEVENTF_MOVE);
        mouse_event_at(link, MOUSEEVENTF_LEFTDOWN);
        mouse_event_at(link, MOUSEEVENTF_LEFTUP);
        browser.expect_page("detail-top");
        for (dx, dy, state) in [
            (-120, 0, "index-top"),
            (120, 0, "detail-top"),
            (0, 120, "detail-end"),
            (0, -120, "detail-top"),
            (-120, -120, "copy-KeyC"),
            (120, 120, "paste-KeyV"),
        ] {
            eprintln!("checking {dx}, {dy} -> {state}");
            browser.swipe(dx, dy);
            browser.expect_page(state);
        }
        // Drive the library through the installed hook. All tabs belong to this test profile.
        let mut pack = glidequill::library::Pack {
            bindings,
            ..Default::default()
        };
        use glidequill::library::Scope;
        pack.set(Scope::Global, "Z", Action::End);
        pack.set(Scope::Global, "UP", Action::NewTab);
        pack.set(
            Scope::Global,
            "DOWN",
            Action::Keys("Ctrl+Shift+T".parse().unwrap()),
        );
        pack.set(Scope::Global, "RIGHT", Action::CloseTab);
        pack.set(Scope::Global, "WheelSwitchUp", Action::PreviousTab);
        pack.set(Scope::Global, "WheelSwitchDown", Action::NextTab);
        std::fs::write(
            browser.dir.join("settings.json"),
            serde_json::to_vec(&pack).unwrap(),
        )
        .unwrap();
        settings::load_from(&browser.dir).unwrap();
        browser.test_path(&[[90, 0], [0, 90], [90, 90]], None);
        browser.expect_page("detail-end");
        browser.swipe(0, -120);
        browser.expect_test_tab(false);
        browser.test_path(&[], Some(120));
        browser.expect_test_tab(true);
        browser.swipe(120, 0);
        browser.expect_test_tab(false);
        browser.swipe(0, 120);
        browser.expect_test_tab(true);
        browser.test_path(&[], Some(-120));
        browser.expect_test_tab(false);
        browser.swipe(120, 0);
        browser.expect_test_tab(true);
        std::fs::write(
            browser.dir.join("settings.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        settings::load_from(&browser.dir).unwrap();
        // A second process owns the foreground while the origin remains visible.
        // Navigation must activate only the browser under the pointer.
        let other = Browser::start("foreground");
        let mut rect = RECT::default();
        GetWindowRect(browser.window, &mut rect);
        let mid_x = (rect.left + rect.right) / 2;
        SetWindowPos(
            other.window,
            HWND_TOP,
            mid_x - 610,
            rect.top,
            500,
            320,
            SWP_SHOWWINDOW,
        );
        browser.activate();
        other.activate();
        browser.swipe_with_foreground(-120, 0, other.window);
        browser.expect_page("index-top");
        assert_eq!(GetForegroundWindow(), browser.window);
        other.expect_page("index-top");
        // Verify a standard frame first, then the same external window with the
        // custom frame flags observed on QcClip (minimize box, no system menu).
        for custom_frame in [false, true] {
            if custom_frame {
                ShowWindowAsync(browser.window, SW_RESTORE);
                let start = Instant::now();
                while IsIconic(browser.window) != 0 && start.elapsed() < Duration::from_secs(2) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let style = GetWindowLongW(browser.window, GWL_STYLE) as u32;
                assert_ne!(style & WS_MINIMIZEBOX, 0);
                assert_ne!(
                    SetWindowLongW(browser.window, GWL_STYLE, (style & !WS_SYSMENU) as i32),
                    0
                );
                assert_eq!(
                    GetWindowLongW(browser.window, GWL_STYLE) as u32 & WS_SYSMENU,
                    0
                );
            }
            browser.activate();
            let mut normal = RECT::default();
            GetWindowRect(browser.window, &mut normal);
            ShowWindowAsync(browser.window, SW_MAXIMIZE);
            let start = Instant::now();
            while IsZoomed(browser.window) == 0 && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_ne!(IsZoomed(browser.window), 0);
            if !custom_frame {
                // Reproduce the stale menu state observed on a real maximized Edge window.
                let menu = GetSystemMenu(browser.window, 0);
                EnableMenuItem(menu, SC_RESTORE, MF_BYCOMMAND | MF_GRAYED);
                assert_ne!(GetMenuState(menu, SC_RESTORE, MF_BYCOMMAND) & MF_GRAYED, 0);
            }
            for want_maximized in [false, true, false] {
                browser.swipe(120, -120);
                let start = Instant::now();
                while (IsZoomed(browser.window) != 0) != want_maximized
                    && start.elapsed() < Duration::from_secs(2)
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert_eq!(
                    IsZoomed(browser.window) != 0,
                    want_maximized,
                    "maximize/restore (custom frame: {custom_frame})"
                );
            }
            let mut restored = RECT::default();
            let start = Instant::now();
            loop {
                GetWindowRect(browser.window, &mut restored);
                if (restored.left, restored.top, restored.right, restored.bottom)
                    == (normal.left, normal.top, normal.right, normal.bottom)
                    || start.elapsed() >= Duration::from_secs(2)
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                (restored.left, restored.top, restored.right, restored.bottom),
                (normal.left, normal.top, normal.right, normal.bottom)
            );
            other.activate();
            browser.swipe_with_foreground(-120, 120, other.window);
            let start = Instant::now();
            while IsIconic(browser.window) == 0 && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(IsIconic(other.window), 0);
            assert_ne!(
                IsIconic(browser.window),
                0,
                "external window did not minimize (custom frame: {custom_frame})"
            );
        }
        ShowWindowAsync(browser.window, SW_RESTORE);
        let start = Instant::now();
        while IsIconic(browser.window) != 0 && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        browser.activate();
        bindings[glidequill::Gesture::DownRight as usize] = Action::Close;
        std::fs::write(
            browser.dir.join("settings.json"),
            serde_json::to_vec(&serde_json::json!({"bindings":bindings})).unwrap(),
        )
        .unwrap();
        settings::load_from(&browser.dir).unwrap();
        browser.swipe(120, 120);
        let start = Instant::now();
        while IsWindow(browser.window) != 0 && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            IsWindow(browser.window),
            0,
            "close must dismiss the disposable window"
        );
    }
}
