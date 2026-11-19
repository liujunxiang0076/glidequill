use crate::input;
use std::{io, ptr::null_mut};
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForSingleObject,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::{
    Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};

/// Dropping this session stops its message loop and waits for hook removal.
pub struct Listener {
    stop: Arc<OwnedHandle>,
    thread: Option<JoinHandle<()>>,
}
impl Listener {
    pub fn start(settings_window: HWND, pass_through: bool) -> io::Result<Self> {
        unsafe {
            let event = CreateEventW(null_mut(), 1, 0, null_mut());
            if event.is_null() {
                return Err(io::Error::last_os_error());
            }
            let stop = Arc::new(OwnedHandle::from_raw_handle(event));
            let worker_stop = Arc::clone(&stop);
            let (ready, started) = mpsc::sync_channel(1);
            // HWND is only an identity for exclusion; this thread never calls UI controls.
            let window_id = settings_window as usize;
            let thread = thread::Builder::new()
                .name("gesture-input".into())
                .spawn(move || {
                    use windows_sys::Win32::UI::HiDpi::{
                        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
                    };
                    SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                    input::initialize(window_id as HWND, pass_through);
                    let hook = SetWindowsHookExW(
                        WH_MOUSE_LL,
                        Some(input::hook),
                        GetModuleHandleW(null_mut()),
                        0,
                    );
                    if hook.is_null() {
                        let _ = ready.send(Err(io::Error::last_os_error()));
                        return;
                    }
                    let focus = SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        null_mut(),
                        Some(input::foreground_changed),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    );
                    if focus.is_null() {
                        let error = io::Error::last_os_error();
                        UnhookWindowsHookEx(hook);
                        let _ = ready.send(Err(error));
                        return;
                    }
                    if ready.send(Ok(())).is_ok() {
                        pump(worker_stop.as_raw_handle());
                    }
                    UnhookWinEvent(focus);
                    UnhookWindowsHookEx(hook);
                })?;
            let session = Self {
                stop,
                thread: Some(thread),
            };
            started
                .recv()
                .map_err(|_| io::Error::other("输入线程启动中断"))??;
            Ok(session)
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        unsafe {
            SetEvent(self.stop.as_raw_handle());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
unsafe fn pump(stop: HANDLE) {
    unsafe {
        let mut msg = MSG::default();
        while MsgWaitForMultipleObjectsEx(1, &stop, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
            == WAIT_OBJECT_0 + 1
        {
            while WaitForSingleObject(stop, 0) == WAIT_TIMEOUT
                && PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0
            {
                if msg.message == WM_QUIT {
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::time::Duration;

    thread_local! { static ACK:RefCell<Option<mpsc::Sender<()>>>=const{RefCell::new(None)}; }

    unsafe extern "system" fn acknowledge(_: HWND, _: u32, id: usize, _: u32) {
        unsafe {
            KillTimer(null_mut(), id);
        }
        ACK.with(|a| {
            if let Some(tx) = a.borrow_mut().take() {
                let _ = tx.send(());
            }
        });
    }

    #[test]
    fn input_message_pump_progresses_while_caller_is_blocked() {
        let raw = unsafe { CreateEventW(null_mut(), 1, 0, null_mut()) };
        assert!(!raw.is_null());
        let stop = Arc::new(unsafe { OwnedHandle::from_raw_handle(raw) });
        let worker_stop = Arc::clone(&stop);
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || unsafe {
            ACK.with(|a| *a.borrow_mut() = Some(tx));
            assert_ne!(SetTimer(null_mut(), 0, 10, Some(acknowledge)), 0);
            pump(worker_stop.as_raw_handle());
        });
        // Deliberately do not dispatch Windows messages on this thread.
        let result = rx.recv_timeout(Duration::from_secs(3));
        unsafe {
            SetEvent(stop.as_raw_handle());
        }
        worker.join().unwrap();
        result.unwrap();
    }
}
