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
struct FeedbackWindow(HWND, HWINEVENTHOOK);
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
            initialize(host, false);
            overlay::create().unwrap();
            use windows_sys::Win32::UI::Accessibility::SetWinEventHook;
            let focus_hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                null_mut(),
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            assert!(!focus_hook.is_null());
            Self(host, focus_hook)
        }
    }
}
impl Drop for FeedbackWindow {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::UI::Accessibility::UnhookWinEvent(self.1);
            overlay::destroy();
            DestroyWindow(self.0);
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
<style>body{height:350vh}a{position:fixed}</style><a autofocus href="#detail">Open detail</a>
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
            // Out-of-context foreground events are delivered to the installing thread.
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
    fn swipe(&self, dx: i32, dy: i32) {
        self.activate();
        self.swipe_with_foreground(dx, dy, self.window);
    }
    fn swipe_with_foreground(&self, dx: i32, dy: i32, foreground: HWND) {
        unsafe {
            assert_eq!(GetForegroundWindow(), foreground);
            let mut rect = RECT::default();
            assert_ne!(GetWindowRect(self.window, &mut rect), 0);
            let center = POINT {
                x: (rect.left + rect.right) / 2,
                y: (rect.top + rect.bottom) / 2,
            };
            let start = Instant::now();
            while GetAncestor(WindowFromPoint(center), GA_ROOT) != self.window
                && start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(GetAncestor(WindowFromPoint(center), GA_ROOT), self.window);
            let end = POINT {
                x: center.x + dx,
                y: center.y + dy,
            };
            assert!(
                gesture(WM_RBUTTONDOWN, center),
                "stroke {dx},{dy}: hit={:?}, target={:?}, zoomed={}, style={:x}",
                GetAncestor(WindowFromPoint(center), GA_ROOT),
                self.window,
                IsZoomed(self.window),
                GetWindowLongW(self.window, GWL_STYLE)
            );
            gesture(WM_MOUSEMOVE, end);
            overlay::refresh(crate::app_window());
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
            assert!(gesture(WM_RBUTTONUP, end));
            overlay::refresh(crate::app_window());
        }
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
        let enter = [0, KEYEVENTF_KEYUP].map(|flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_RETURN,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        });
        assert_eq!(SendInput(2, enter.as_ptr(), size_of::<INPUT>() as i32), 2);
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
