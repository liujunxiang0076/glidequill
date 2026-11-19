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
struct Browser {
    child: Child,
    dir: PathBuf,
    window: HWND,
}
impl Browser {
    fn start() -> Self {
        let chrome = std::env::var_os("GLIDEQUILL_TEST_BROWSER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("ProgramFiles").unwrap())
                    .join("Google/Chrome/Application/chrome.exe")
            });
        let dir = std::env::temp_dir().join(format!("glidequill-browser-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let page = dir.join("test.html");
        std::fs::write(
            &page,
            "<!doctype html><title>GlideQuill minimize test</title>",
        )
        .unwrap();
        let child = Command::new(chrome)
            .arg(format!("--user-data-dir={}", dir.join("profile").display()))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-mode",
                "--window-size=740,560",
            ])
            .arg(format!(
                "--app=file:///{}",
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
            if pid == browser.child.id() && title(window).contains("GlideQuill minimize test") {
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
    fn swipe_down_left(&self) {
        unsafe {
            assert_eq!(GetForegroundWindow(), self.window);
            let mut rect = RECT::default();
            assert_ne!(GetWindowRect(self.window, &mut rect), 0);
            let center = POINT {
                x: (rect.left + rect.right) / 2,
                y: (rect.top + rect.bottom) / 2,
            };
            let end = POINT {
                x: center.x - 120,
                y: center.y + 120,
            };
            assert!(gesture(WM_RBUTTONDOWN, center));
            gesture(WM_MOUSEMOVE, end);
            assert!(gesture(WM_RBUTTONUP, end));
        }
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        if !self.window.is_null() {
            unsafe {
                PostMessageW(self.window, WM_CLOSE, 0, 0);
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
fn down_left_minimizes_external_window() {
    let _desktop = DESKTOP_TEST.lock().unwrap();
    let browser = Browser::start();
    let mut bindings = glidequill::DEFAULT_BINDINGS;
    bindings[glidequill::Gesture::DownLeft as usize] = Action::Minimize;
    let config = serde_json::json!({"bindings": bindings});
    std::fs::write(
        browser.dir.join("settings.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    settings::load_from(&browser.dir).unwrap();
    unsafe {
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
            SetForegroundWindow(browser.window);
            browser.swipe_down_left();
            let start = Instant::now();
            while IsIconic(browser.window) == 0 && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_ne!(
                IsIconic(browser.window),
                0,
                "external window did not minimize (custom frame: {custom_frame})"
            );
        }
    }
}
