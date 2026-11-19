use super::*;
use std::time::{Duration, Instant};
use windows_sys::Win32::{Graphics::Gdi::*, System::Threading::*, UI::Input::KeyboardAndMouse::*};

struct Capture {
    target: HWND,
    browser: bool,
    stroke: Stroke,
    last: Instant,
    expired: bool,
}
thread_local! {static CAPTURE:RefCell<Option<Capture>>=const{RefCell::new(None)};}
pub fn cancel() {
    CAPTURE.with(|c| c.borrow_mut().take());
}

unsafe fn eligible(point: POINT) -> Option<(HWND, bool)> {
    unsafe {
        let target = GetAncestor(WindowFromPoint(point), GA_ROOT);
        if target.is_null() || target != GetForegroundWindow() || target == app_window() {
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
        let br = glidequill::browser(exe);
        let c = settings::snapshot();
        let skip = {
            c.excluded.iter().any(|s| s.eq_ignore_ascii_case(exe))
                || DIRECTIONS
                    .iter()
                    .all(|&d| matches!(c.resolve(br, d), Action::Off | Action::Inherit))
        };
        (!skip).then_some((target, br))
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
        if code < 0 {
            return CallNextHookEx(null_mut(), code, msg, data);
        }
        let e = &*(data as *const MSLLHOOKSTRUCT);
        if e.flags & LLMHF_INJECTED != 0 {
            return CallNextHookEx(null_mut(), code, msg, data);
        }
        let swallowed = match msg as u32 {
            WM_RBUTTONDOWN => {
                if [
                    VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, VK_LBUTTON, VK_MBUTTON,
                ]
                .iter()
                .any(|&k| GetAsyncKeyState(k as i32) < 0)
                    || GetSystemMetrics(SM_SWAPBUTTON) != 0
                {
                    false
                } else if let Some((target, browser)) = eligible(e.pt) {
                    CAPTURE.with(|c| {
                        *c.borrow_mut() = Some(Capture {
                            target,
                            browser,
                            stroke: Stroke::new(e.pt.x, e.pt.y),
                            last: Instant::now(),
                            expired: false,
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
                        s.expired |= s.last.elapsed() > Duration::from_millis(750);
                        s.last = Instant::now();
                        s.stroke.sample(e.pt.x, e.pt.y);
                    }
                });
                false
            }
            WM_RBUTTONUP => {
                if let Some(mut s) = CAPTURE.with(|c| c.borrow_mut().take()) {
                    s.stroke.sample(e.pt.x, e.pt.y);
                    if s.target == GetForegroundWindow() {
                        if s.stroke.is_click() {
                            if GetAncestor(WindowFromPoint(e.pt), GA_ROOT) == s.target {
                                replay_click();
                            }
                        } else if !s.expired
                            && s.last.elapsed() <= Duration::from_millis(750)
                            && let Some(d) = s.stroke.direction()
                        {
                            PostMessageW(
                                app_window(),
                                RUN,
                                s.target as usize,
                                (d as isize) | ((s.browser as isize) << 8),
                            );
                        }
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if swallowed {
            1
        } else {
            CallNextHookEx(null_mut(), code, msg, data)
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
pub unsafe fn perform(target: HWND, a: Action) {
    unsafe {
        let system_command = match a {
            Action::Minimize => Some(SC_MINIMIZE),
            Action::Maximize => Some(if IsZoomed(target) != 0 {
                SC_RESTORE
            } else {
                SC_MAXIMIZE
            }),
            Action::Close => Some(SC_CLOSE),
            _ => None,
        };
        if let Some(command) = system_command {
            PostMessageW(target, WM_SYSCOMMAND, command as usize, 0);
            return;
        }
        let (modifier, key) = match a {
            Action::Back => (VK_MENU, VK_LEFT),
            Action::Forward => (VK_MENU, VK_RIGHT),
            Action::Copy => (VK_CONTROL, 0x43),
            Action::Paste => (VK_CONTROL, 0x56),
            Action::Home => (VK_CONTROL, VK_HOME),
            Action::End => (VK_CONTROL, VK_END),
            _ => return,
        };
        if [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&k| GetAsyncKeyState(k as i32) < 0)
        {
            return;
        }
        let batch = [
            (modifier, 0),
            (key, 0),
            (key, KEYEVENTF_KEYUP),
            (modifier, KEYEVENTF_KEYUP),
        ]
        .map(|(k, f)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: k,
                    dwFlags: f,
                    ..Default::default()
                },
            },
        });
        SendInput(4, batch.as_ptr(), size_of::<INPUT>() as i32);
    }
}
