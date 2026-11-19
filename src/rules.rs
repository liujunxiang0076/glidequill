use super::*;
use settings::{Applications, Bindings};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};

pub const SCOPE: i32 = 192;
const GLOBAL: i32 = 193;
const ADD: i32 = 194;
const REMOVE: i32 = 195;
const FIRST_APPLICATION: i32 = 300;
const INHERIT: usize = ACTIONS.len();

struct Draft {
    global: Bindings,
    apps: Applications,
    scope: Option<String>,
    library: glidequill::library::Library,
}
thread_local! {
    static DRAFT: RefCell<Draft> = RefCell::new(Draft {
        global: settings::preferences().bindings,
        apps: settings::applications(),
        scope: None,
        library:settings::pack().library,
    });
}

pub fn is_application() -> bool {
    DRAFT.with_borrow(|d| d.scope.is_some())
}
pub fn clear_index() -> usize {
    if is_application() {
        INHERIT
    } else {
        Action::Unassigned.index()
    }
}
fn inherited_label(d: &Draft, i: usize) -> String {
    if let Some(app) = &d.scope
        && let Some(action) = d.library.action(app, glidequill::library::SIGNS[i])
    {
        return format!("跟随应用组 · {}", action.display());
    }
    format!("跟随通用 · {}", d.global[i].display())
}
pub fn option_label(id: i32, option: usize) -> Option<String> {
    if let Some(a) = ACTIONS.get(option) {
        return Some(a.label().into());
    }
    let i = (id - ui::PICKER) as usize;
    DRAFT.with_borrow(|d| {
        if d.scope.is_some() && option == INHERIT {
            return Some(inherited_label(d, i));
        }
        let a = if let Some(name) = &d.scope {
            d.apps[name][i]
        } else {
            Some(d.global[i])
        };
        a.filter(|a| matches!(a, Action::Keys(_)))
            .map(Action::display)
    })
}
pub fn field_changed(id: i32) {
    let selection = unsafe { SendMessageW(GetDlgItem(app_window(), id), CB_GETCURSEL, 0, 0) };
    if selection < 0 {
        return;
    }
    let index = (id - ui::PICKER) as usize;
    let action = ACTIONS.get(selection as usize).copied();
    if action.is_none() && !(is_application() && selection as usize == INHERIT) {
        return;
    }
    DRAFT.with_borrow_mut(|d| {
        if let Some(bindings) = d.scope.as_ref().and_then(|name| d.apps.get_mut(name)) {
            bindings[index] = action;
        } else if let Some(action) = action {
            d.global[index] = action;
        }
    });
    update_buttons();
}
// Native controls may synchronously paint during SendMessage. Release the draft borrow first.
pub fn show_scope() {
    let (name, global, overrides) = DRAFT.with_borrow(|d| {
        (
            d.scope.clone(),
            d.global,
            d.scope.as_ref().map(|name| d.apps[name]),
        )
    });
    label(
        SCOPE,
        &format!("{}  ▾", name.as_deref().unwrap_or("通用规则")),
    );
    for (i, action) in global.into_iter().enumerate() {
        unsafe {
            let field = GetDlgItem(app_window(), ui::PICKER + i as i32);
            while SendMessageW(field, CB_GETCOUNT, 0, 0) > INHERIT as isize {
                SendMessageW(field, CB_DELETESTRING, INHERIT, 0);
            }
            let selection = if let Some(bindings) = overrides {
                let text = wide(&DRAFT.with_borrow(|d| inherited_label(d, i)));
                SendMessageW(field, CB_ADDSTRING, 0, text.as_ptr() as isize);
                if let Some(Action::Keys(c)) = bindings[i] {
                    SendMessageW(
                        field,
                        CB_ADDSTRING,
                        0,
                        wide(&c.to_string()).as_ptr() as isize,
                    ) as usize
                } else {
                    bindings[i].map_or(INHERIT, |a| a.index())
                }
            } else if let Action::Keys(c) = action {
                SendMessageW(
                    field,
                    CB_ADDSTRING,
                    0,
                    wide(&c.to_string()).as_ptr() as isize,
                ) as usize
            } else {
                action.index()
            };
            SendMessageW(field, CB_SETCURSEL, selection, 0);
        }
    }
    if let Some(name) = name {
        label(
            RESULT,
            if settings::excluded(&name) {
                "此程序已被排除，移出排除列表后才会使用这些规则"
            } else {
                "按程序文件名匹配，修改后点击「保存并重载」"
            },
        );
    } else {
        label(RESULT, "通用规则适用于所有未单独指定动作的程序");
    }
    unsafe {
        InvalidateRect(app_window(), null_mut(), 0);
    }
    update_buttons();
}
pub fn update_buttons() {
    let (count, reset) = DRAFT.with_borrow(|d| {
        let range = rule_range();
        let can_reset = match &d.scope {
            Some(name) => d.apps[name][range].iter().any(Option::is_some),
            None => d.global[range.clone()] != DEFAULT_BINDINGS[range],
        };
        (
            settings::rule_changes(&d.global, &d.apps)
                + usize::from(!settings::library_matches(&d.library)),
            can_reset,
        )
    });
    label(
        DIRTY_RULES,
        &if count > 0 {
            format!("有 {count} 项未保存")
        } else {
            String::new()
        },
    );
    unsafe {
        for (id, changed) in [(RESET_RULES, reset), (SAVE_RULES, count > 0)] {
            let w = GetDlgItem(app_window(), id);
            let enabled = changed && CONFIG_READABLE.get();
            if (IsWindowEnabled(w) != 0) != enabled {
                EnableWindow(w, enabled.into());
                InvalidateRect(w, null_mut(), 0);
            }
        }
    }
}
pub fn reset_group() {
    let app = is_application();
    for i in rule_range() {
        let selection = if app {
            INHERIT
        } else {
            DEFAULT_BINDINGS[i].index()
        };
        unsafe {
            SendMessageW(
                GetDlgItem(app_window(), ui::PICKER + i as i32),
                CB_SETCURSEL,
                selection,
                0,
            );
        }
    }
    DRAFT.with_borrow_mut(|d| {
        let range = rule_range();
        if let Some(name) = &d.scope {
            d.apps.get_mut(name).unwrap()[range].fill(None);
        } else {
            d.global[range.clone()].copy_from_slice(&DEFAULT_BINDINGS[range]);
        }
    });
    update_buttons();
}
pub fn save() {
    match settings::save_pack(&draft_pack()) {
        Ok(()) => label(RESULT, "规则已保存并重载，将用于下一笔手势"),
        Err(e) => label(RESULT, &format!("保存失败，修改已保留：{e}")),
    }
    update_buttons();
}
pub fn menu(owner: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        DRAFT.with_borrow(|d| {
            let checked = |active| if active { MF_CHECKED } else { MF_STRING };
            AppendMenuW(
                menu,
                checked(d.scope.is_none()),
                GLOBAL as usize,
                wide("通用规则").as_ptr(),
            );
            for (i, name) in d.apps.keys().enumerate() {
                AppendMenuW(
                    menu,
                    checked(d.scope.as_ref() == Some(name)),
                    FIRST_APPLICATION as usize + i,
                    wide(&name.replace('&', "&&")).as_ptr(),
                );
            }
            AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
            AppendMenuW(menu, MF_STRING, ADD as usize, wide("添加应用…").as_ptr());
            if d.scope.is_some() {
                AppendMenuW(
                    menu,
                    MF_STRING,
                    REMOVE as usize,
                    wide("移除当前应用规则").as_ptr(),
                );
            }
        });
        let mut r = RECT::default();
        GetWindowRect(GetDlgItem(owner, SCOPE), &mut r);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            r.left,
            r.bottom,
            0,
            owner,
            null_mut(),
        );
        DestroyMenu(menu);
        scope_command(command);
    }
}
pub fn is_scope_command(id: i32) -> bool {
    matches!(id, GLOBAL | ADD | REMOVE) || id >= FIRST_APPLICATION
}
pub fn scope_command(id: i32) {
    let scope = match id {
        GLOBAL => None,
        ADD => {
            let Some(name) = exclusions::choose_executable(app_window(), "选择要设置手势的应用")
            else {
                return;
            };
            let name = name.to_lowercase();
            DRAFT.with_borrow_mut(|d| {
                d.apps.entry(name.clone()).or_insert([None; GESTURES.len()]);
            });
            Some(name)
        }
        REMOVE => {
            DRAFT.with_borrow_mut(|d| {
                if let Some(name) = d.scope.take() {
                    d.apps.remove(&name);
                }
            });
            None
        }
        id if id >= FIRST_APPLICATION => {
            let name = DRAFT.with_borrow(|d| {
                d.apps
                    .keys()
                    .nth((id - FIRST_APPLICATION) as usize)
                    .cloned()
            });
            if name.is_none() {
                return;
            }
            name
        }
        _ => return,
    };
    DRAFT.with_borrow_mut(|d| d.scope = scope);
    show_scope();
}

pub fn draft_pack() -> glidequill::library::Pack {
    DRAFT.with_borrow(|d| glidequill::library::Pack {
        bindings: d.global,
        applications: d.apps.clone(),
        library: d.library.clone(),
    })
}
pub fn replace_draft(pack: glidequill::library::Pack) {
    DRAFT.with_borrow_mut(|d| {
        d.global = pack.bindings;
        d.apps = pack.applications;
        d.library = pack.library;
        if d.scope
            .as_ref()
            .is_some_and(|name| !d.apps.contains_key(name))
        {
            d.scope = None;
        }
    });
    show_scope();
}
