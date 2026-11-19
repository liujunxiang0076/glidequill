use crate::wide;
use std::{io, os::windows::ffi::OsStrExt, ptr::null_mut};
use windows_sys::Win32::{Foundation::*, System::Registry::*};

thread_local! {static KEY: std::cell::RefCell<String>=std::cell::RefCell::new(String::from(r"Software\Microsoft\Windows\CurrentVersion\Run"));}
pub fn isolate_test(id: &str) {
    KEY.with(|k| *k.borrow_mut() = format!(r"Software\GlideQuill\Tests\{id}"));
}
fn command() -> io::Result<Vec<u16>> {
    let mut result = vec![b'"' as u16];
    result.extend(std::env::current_exe()?.as_os_str().encode_wide());
    result.extend(wide("\" --startup"));
    Ok(result)
}
pub fn registered() -> io::Result<bool> {
    let key = KEY.with(|k| wide(&k.borrow()));
    let expected = command()?;
    let mut data = vec![0u16; 32768];
    let mut size = (data.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            wide("GlideQuill").as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(data[..size as usize / 2] == expected),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        e => Err(io::Error::from_raw_os_error(e as i32)),
    }
}
pub fn set(enabled: bool) -> io::Result<()> {
    let key = KEY.with(|k| wide(&k.borrow()));
    let name = wide("GlideQuill");
    let status = if enabled {
        let value = command()?;
        if value.len() > 260 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "程序路径太长，请移至较短路径后重试",
            ));
        }
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        }
    } else {
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) }
    };
    if status == ERROR_SUCCESS
        || (!enabled && matches!(status, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND))
    {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}
