use super::*;
use std::path::Path;
use windows_sys::Win32::UI::{
    Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    },
    Input::KeyboardAndMouse::EnableWindow,
};

thread_local! {
    static DRAFT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn list() -> HWND {
    unsafe { GetDlgItem(app_window(), APP_LIST) }
}
fn dirty() -> bool {
    DRAFT.with_borrow(|names| *names != settings::exclusions())
}
pub fn update_buttons() {
    unsafe {
        EnableWindow(GetDlgItem(app_window(), SAVE_APPS), dirty().into());
        EnableWindow(
            GetDlgItem(app_window(), REMOVE_APP),
            (SendMessageW(list(), LB_GETCURSEL, 0, 0) >= 0).into(),
        );
    }
}
fn render(selected: Option<usize>) {
    unsafe {
        let list = list();
        SendMessageW(list, WM_SETREDRAW, 0, 0);
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        DRAFT.with_borrow(|names| {
            for name in names {
                SendMessageW(list, LB_ADDSTRING, 0, wide(name).as_ptr() as isize);
            }
        });
        if let Some(index) = selected {
            SendMessageW(list, LB_SETCURSEL, index, 0);
        }
        SendMessageW(list, WM_SETREDRAW, 1, 0);
        InvalidateRect(list, null_mut(), 1);
    }
    update_buttons();
}
pub fn open() {
    DRAFT.replace(settings::exclusions());
    render(None);
    label(
        RESULT,
        if DRAFT.with_borrow(Vec::is_empty) {
            "尚未排除应用，点击「添加应用」选择程序"
        } else {
            "在这些程序中，右键始终使用原生操作"
        },
    );
}
pub unsafe fn pick(owner: HWND) {
    let mut buffer = vec![0u16; 32768];
    let filter = wide("应用程序 (*.exe)\0*.exe\0");
    let title = wide("选择要排除的应用");
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrFile: buffer.as_mut_ptr(),
        nMaxFile: buffer.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        let err = unsafe { CommDlgExtendedError() };
        if err != 0 {
            label(RESULT, &format!("无法选择应用 ({err})"));
        }
        return;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let filename = String::from_utf16_lossy(&buffer[..len]);
    let path = Path::new(&filename);
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        label(RESULT, "请选择 .exe 程序文件");
        return;
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let (index, added) = DRAFT.with_borrow_mut(|names| {
            if let Some(index) = names.iter().position(|s| s.eq_ignore_ascii_case(name)) {
                return (index, false);
            }
            names.push(name.to_owned());
            (names.len() - 1, true)
        });
        render(Some(index));
        label(
            RESULT,
            if added {
                "应用已加入草稿，保存后生效"
            } else {
                "此应用已在列表中"
            },
        );
    }
}
pub fn remove() {
    let index = unsafe { SendMessageW(list(), LB_GETCURSEL, 0, 0) };
    if index < 0 {
        return;
    }
    let next = DRAFT.with_borrow_mut(|names| {
        names.remove(index as usize);
        names
            .len()
            .checked_sub(1)
            .map(|last| last.min(index as usize))
    });
    render(next);
    label(RESULT, "应用已从草稿移除，保存后生效");
}
pub fn save() -> bool {
    let result = DRAFT.with_borrow(|names| settings::save_exclusions(names));
    match result {
        Ok(()) => {
            update_buttons();
            label(
                EXCLUSIONS,
                &format!("排除的应用 · {} 个应用", settings::exclusions().len()),
            );
            label(RESULT, "排除列表已保存，从下一笔手势生效");
            true
        }
        Err(e) => {
            label(RESULT, &format!("保存失败，修改已保留：{e}"));
            false
        }
    }
}
pub unsafe fn may_leave(owner: HWND) -> bool {
    if !dirty() {
        return true;
    }
    let choice = unsafe {
        MessageBoxW(
            owner,
            wide("保存排除列表的修改吗？").as_ptr(),
            wide("排除的应用").as_ptr(),
            MB_YESNOCANCEL | MB_ICONQUESTION,
        )
    };
    match choice {
        IDYES => save(),
        IDNO => true,
        _ => false,
    }
}
