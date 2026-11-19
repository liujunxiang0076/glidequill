use crate::{overlay, settings};
use glidequill::Stroke;

use std::{
    cell::{Cell, RefCell},
    mem::size_of,
    ptr::null_mut,
};
use windows_sys::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
use windows_sys::Win32::{Graphics::Gdi::*, System::Threading::*, UI::Input::KeyboardAndMouse::*};

struct Capture {
    target: HWND,
    origin: POINT,
    dpi: u32,
    stroke: Stroke,
}
impl Capture {
    fn sample(&mut self, point: POINT) {
        let scale = 96.0 / self.dpi as f64;
        self.stroke.sample(
            ((point.x - self.origin.x) as f64 * scale).round() as i32,
            ((point.y - self.origin.y) as f64 * scale).round() as i32,
        );
    }
}
thread_local! {
    static CAPTURE:RefCell<Option<Capture>>=const{RefCell::new(None)};
    static SETTINGS_WINDOW:Cell<HWND>=const{Cell::new(null_mut())};
    static PASS_THROUGH:Cell<bool>=const{Cell::new(false)};
}
pub fn initialize(window: HWND, pass_through: bool) {
    SETTINGS_WINDOW.with(|w| w.set(window));
    PASS_THROUGH.with(|v| v.set(pass_through));
}
unsafe fn eligible(point: POINT) -> Option<HWND> {
    unsafe {
        let target = GetAncestor(WindowFromPoint(point), GA_ROOT);
        if target.is_null() || target != GetForegroundWindow() {
            return None;
        }
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut rect = RECT::default();
        if GetMonitorInfoW(MonitorFromWindow(target, MONITOR_DEFAULTTONEAREST), &mut mi) != 0
            && GetWindowRect(target, &mut rect) != 0
            && rect.left <= mi.rcMonitor.left
            && rect.top <= mi.rcMonitor.top
            && rect.right >= mi.rcMonitor.right
            && rect.bottom >= mi.rcMonitor.bottom
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
        (!settings::excluded(exe)).then_some(target)
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
pub unsafe extern "system" fn hook(code: i32, msg: WPARAM, data: LPARAM) -> LRESULT {
    unsafe {
        if code < 0 || PASS_THROUGH.with(Cell::get) {
            return CallNextHookEx(null_mut(), code, msg, data);
        }
        let e = &*(data as *const MSLLHOOKSTRUCT);
        if e.flags & LLMHF_INJECTED != 0 {
            return CallNextHookEx(null_mut(), code, msg, data);
        }
        if gesture(msg as u32, e.pt) {
            1
        } else {
            CallNextHookEx(null_mut(), code, msg, data)
        }
    }
}
unsafe fn gesture(msg: u32, point: POINT) -> bool {
    unsafe {
        match msg {
            WM_RBUTTONDOWN => {
                if [
                    VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, VK_LBUTTON, VK_MBUTTON,
                ]
                .iter()
                .any(|&k| GetAsyncKeyState(k as i32) < 0)
                    || GetSystemMetrics(SM_SWAPBUTTON) != 0
                {
                    false
                } else if let Some(target) = eligible(point) {
                    let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(target).max(96);
                    overlay::begin(SETTINGS_WINDOW.with(Cell::get), target, point, dpi);
                    CAPTURE.with(|c| {
                        *c.borrow_mut() = Some(Capture {
                            target,
                            origin: point,
                            dpi,
                            stroke: Stroke::new(0, 0),
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
                        s.sample(point);
                        if s.target != GetForegroundWindow() {
                            overlay::cancel(SETTINGS_WINDOW.with(Cell::get));
                        } else if !s.stroke.is_click() {
                            overlay::extend(SETTINGS_WINDOW.with(Cell::get), point);
                        }
                    }
                });
                false
            }
            WM_RBUTTONUP => {
                if let Some(mut s) = CAPTURE.with(|c| c.borrow_mut().take()) {
                    s.sample(point);
                    let window = SETTINGS_WINDOW.with(Cell::get);
                    if s.target != GetForegroundWindow() {
                        overlay::cancel(window);
                    } else if s.stroke.is_click() {
                        overlay::cancel(window);
                        if GetAncestor(WindowFromPoint(point), GA_ROOT) == s.target {
                            replay_click();
                        }
                    } else {
                        overlay::finish(window, point, s.stroke.direction());
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
mod tests {
    use super::*;
    use crate::{ui, wide};
    // Runs on an interactive desktop. Feed physical-coordinate samples through the
    // production gesture handler without moving the user's pointer or injecting clicks.
    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn native_trail_shows_before_release_and_preserves_the_foreground() {
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
            let start = POINT { x: 280, y: 240 };
            for (x, y) in [(180, 240), (380, 240), (280, 140), (280, 340), (370, 330)] {
                let end = POINT { x, y };
                assert_eq!(
                    eligible(start),
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
                assert_eq!(GetForegroundWindow(), hwnd);
                assert_eq!(GetAncestor(WindowFromPoint(end), GA_ROOT), hwnd);
                assert!(gesture(WM_RBUTTONUP, end));
                overlay::refresh(hwnd);
                let mut rect = RECT::default();
                GetWindowRect(popup, &mut rect);
                assert!(
                    rect.right > end.x + 50,
                    "result badge extends beyond endpoint"
                );
                assert_eq!(GetForegroundWindow(), hwnd);
            }
            std::thread::sleep(std::time::Duration::from_millis(600));
            overlay::refresh(hwnd);
            assert_eq!(IsWindowVisible(popup), 0);
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

            // With idle cancellation disabled the same pause keeps the trail. Zero
            // release time must hide it on the first frame after release.
            let dir =
                std::env::temp_dir().join(format!("glidequill-timing-{}", std::process::id()));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("settings.json"), r#"{"idle_ms":0,"result_ms":0}"#).unwrap();
            settings::load_from(&dir).unwrap();
            std::fs::remove_file(dir.join("settings.json")).unwrap();
            std::fs::remove_dir(dir).unwrap();
            assert!(gesture(WM_RBUTTONDOWN, start));
            gesture(WM_MOUSEMOVE, end);
            overlay::refresh(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(600));
            overlay::refresh(hwnd);
            assert_ne!(IsWindowVisible(popup), 0);
            assert!(gesture(WM_RBUTTONUP, end));
            overlay::refresh(hwnd);
            assert_eq!(IsWindowVisible(popup), 0);
            overlay::destroy();
            DestroyWindow(hwnd);
        }
    }
}
