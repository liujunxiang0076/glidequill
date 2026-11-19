use super::*;
use glidequill::library::{self, Pack, SIGNS, Scope};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicPtr, Ordering},
};
use windows_sys::Win32::UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::*};

pub const OPEN_LIBRARY: i32 = 198;
const GLOBAL: &str = "通用规则";
const GROUP: &str = "组 · ";
static RECORD_WINDOW: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
pub fn recording_window() -> HWND {
    RECORD_WINDOW.load(Ordering::Relaxed)
}
thread_local! {
    static EDIT:RefCell<Pack>=RefCell::new(Pack::default());
    static PREVIEW: RefCell<(String,String)> = const {RefCell::new((String::new(),String::new()))};
    static FILTER:RefCell<Vec<(String,bool)>>=const {RefCell::new(Vec::new())};
    static DRAW:RefCell<library::Path>=const {RefCell::new(Vec::new())};
    static DRAWING:Cell<bool>=const {Cell::new(false)};
    static NAME:RefCell<String>=const {RefCell::new(String::new())};
}
fn dlg(owner: HWND, id: u16, proc: DLGPROC) -> isize {
    unsafe {
        DialogBoxParamW(
            GetModuleHandleW(null_mut()),
            id as usize as *const u16,
            owner,
            proc,
            0,
        )
    }
}
fn field(w: HWND, id: i32) -> String {
    unsafe {
        let h = GetDlgItem(w, id);
        let n = GetWindowTextLengthW(h);
        let mut text = vec![0; (n + 1) as usize];
        let len = GetWindowTextW(h, text.as_mut_ptr(), n + 1);
        String::from_utf16_lossy(&text[..len as usize])
            .trim()
            .to_owned()
    }
}
fn set(w: HWND, id: i32, text: &str) {
    unsafe {
        SetDlgItemTextW(w, id, wide(text).as_ptr());
    }
}
fn msg(w: HWND, id: i32, m: u32, wp: usize, lp: isize) -> isize {
    unsafe { SendDlgItemMessageW(w, id, m, wp, lp) }
}
fn combo(w: HWND, id: i32, items: impl IntoIterator<Item = String>, value: &str) {
    msg(w, id, CB_RESETCONTENT, 0, 0);
    for item in items {
        msg(w, id, CB_ADDSTRING, 0, wide(&item).as_ptr() as isize);
    }
    set(w, id, value);
}
fn scope(w: HWND) -> Result<Scope, String> {
    let name = field(w, 510);
    if name == GLOBAL {
        Ok(Scope::Global)
    } else if let Some(g) = name.strip_prefix(GROUP) {
        if EDIT.with_borrow(|p| p.library.groups.contains_key(g)) {
            Ok(Scope::Group(g.into()))
        } else {
            Err("请先保存应用组".into())
        }
    } else if library::valid_app(&name.to_lowercase()) {
        Ok(Scope::App(name.to_lowercase()))
    } else {
        Err("请选择一个范围，或输入程序文件名，例如 msedge.exe".into())
    }
}
fn picked(pack: &Pack, scope: &Scope, sign: &str) -> Option<Action> {
    if let Some(i) = SIGNS.iter().position(|&s| s == sign) {
        match scope {
            Scope::Global => return Some(pack.bindings[i]),
            Scope::App(a) => return pack.applications.get(a).and_then(|v| v[i]),
            _ => {}
        }
    }
    pack.library
        .rules
        .iter()
        .find(|r| &r.scope == scope && r.sign == sign)
        .map(|r| r.action)
}
fn show_binding(w: HWND) {
    let Ok(scope) = scope(w) else { return };
    let sign = field(w, 511);
    let (action, members) = EDIT.with_borrow(|p| {
        (
            picked(p, &scope, &sign),
            match &scope {
                Scope::Group(g) => p.library.groups.get(g).map(|v| v.join(", ")),
                _ => None,
            },
        )
    });
    set(w, 512, &action.map(Action::display).unwrap_or_default());
    if let Scope::Group(g) = scope {
        set(w, 516, &g);
        set(w, 517, &members.unwrap_or_default());
    }
    set(
        w,
        529,
        if action.is_some() {
            "修改后点「设置绑定」，返回规则页保存。"
        } else {
            "此范围沿用上一级绑定。可选择动作，也可输入 Ctrl+Shift+T 等快捷键。"
        },
    );
}
fn refresh(w: HWND, scope: &str, sign: &str) {
    let (mut scopes, shapes, tuning) = EDIT.with_borrow(|p| {
        let mut scopes = vec![GLOBAL.into()];
        scopes.extend(p.library.groups.keys().map(|g| format!("{GROUP}{g}")));
        scopes.extend(p.applications.keys().cloned());
        scopes.extend(p.library.rules.iter().filter_map(|r| match &r.scope {
            Scope::App(a) => Some(a.clone()),
            _ => None,
        }));
        let mut shapes: Vec<_> = SIGNS.iter().map(|v| v.to_string()).collect();
        shapes.extend(p.library.shapes.keys().cloned());
        if !p.library.template_order.is_empty() {
            shapes.sort_by_key(|n| {
                p.library
                    .template_order
                    .iter()
                    .position(|s| s == n)
                    .unwrap_or(usize::MAX)
            });
        }
        shapes.extend(library::WHEEL.map(str::to_owned));
        (scopes, shapes, p.library.tuning)
    });
    let mut seen = std::collections::BTreeSet::new();
    scopes.retain(|name| seen.insert(name.clone()));
    combo(w, 510, scopes, scope);
    combo(w, 511, shapes, sign);
    combo(w, 512, ACTIONS.iter().map(|a| a.display()), "");
    set(w, 520, &tuning.distance.to_string());
    set(w, 521, &tuning.tolerance.to_string());
    msg(w, 522, BM_SETCHECK, usize::from(tuning.trail), 0);
    show_binding(w);
}
fn tuning(w: HWND) -> Result<(), String> {
    let distance = field(w, 520)
        .parse()
        .ok()
        .filter(|v| (12..=96).contains(v))
        .ok_or("触发距离范围为 12–96 DIP")?;
    let tolerance = field(w, 521)
        .parse()
        .ok()
        .filter(|v| (5..=22).contains(v))
        .ok_or("模板容差范围为 5–22%")?;
    let trail = msg(w, 522, BM_GETCHECK, 0, 0) != 0;
    EDIT.with_borrow_mut(|p| {
        p.library.tuning = library::Tuning {
            distance,
            tolerance,
            trail,
        }
    });
    Ok(())
}
fn commit_edit(f: impl FnOnce(&mut Pack) -> Result<(), String>) -> Result<(), String> {
    let mut next = EDIT.with_borrow(Clone::clone);
    f(&mut next)?;
    next.validate()?;
    EDIT.replace(next);
    Ok(())
}
fn file(w: HWND, save: bool) -> Result<Option<PathBuf>, String> {
    let filter = wide("JSON 配置 (*.json)\0*.json\0");
    let mut path = vec![0; 32768];
    let ext = wide("json");
    let title = wide(if save {
        "导出 GlideQuill 规则包"
    } else {
        "打开 MouseInc.json 或 GlideQuill 规则包"
    });
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: w,
        lpstrTitle: title.as_ptr(),
        lpstrFilter: filter.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        lpstrDefExt: ext.as_ptr(),
        nMaxFile: path.len() as u32,
        Flags: OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            },
        ..Default::default()
    };
    let ok = unsafe {
        if save {
            GetSaveFileNameW(&mut ofn)
        } else {
            GetOpenFileNameW(&mut ofn)
        }
    };
    if ok == 0 {
        let e = unsafe { CommDlgExtendedError() };
        return if e == 0 {
            Ok(None)
        } else {
            Err(format!("文件选择失败 ({e})"))
        };
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(Some(
        std::ffi::OsString::from_wide(&path[..path.iter().position(|&v| v == 0).unwrap()]).into(),
    ))
}
fn preview(w: HWND, title: &str, lines: String) -> bool {
    PREVIEW.replace((title.into(), lines));
    dlg(w, 403, Some(report)) == 1
}
unsafe extern "system" fn report(w: HWND, m: u32, wp: usize, _: isize) -> isize {
    match m {
        WM_INITDIALOG => {
            PREVIEW.with_borrow(|(t, l)| {
                set(w, 540, t);
                set(w, 541, l);
            });
            1
        }
        WM_COMMAND if matches!(wp as u16, 1 | 2) => {
            unsafe { EndDialog(w, wp as u16 as isize) };
            1
        }
        WM_CLOSE => {
            unsafe { EndDialog(w, 2) };
            1
        }
        _ => 0,
    }
}
fn export(path: &Path, pack: &Pack) -> Result<(), String> {
    use std::io::Write;
    let data = serde_json::to_vec_pretty(&serde_json::json!({"glidequill_rules":1,"rules":pack}))
        .map_err(|e| e.to_string())?;
    let temp = path.with_file_name(format!(
        ".glidequill-{}-{}.tmp",
        std::process::id(),
        unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount() }
    ));
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let result = out.write_all(&data).and_then(|()| out.sync_all());
    drop(out);
    result
        .and_then(|()| fs::rename(&temp, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
        .map_err(|e| e.to_string())
}
fn command(w: HWND, id: i32) -> Result<(), String> {
    if id != 2 {
        tuning(w)?;
    }
    let sign = field(w, 511);
    match id {
        1 | 2 => {
            unsafe { EndDialog(w, id as isize) };
        }
        513..=515 => {
            let scope = scope(w)?;
            let value = field(w, 512);
            commit_edit(|p| {
                if !SIGNS.contains(&sign.as_str())
                    && !library::WHEEL.contains(&sign.as_str())
                    && !p.library.shapes.contains_key(&sign)
                {
                    return Err("请先选择或录制手势".into());
                }
                if id == 515 {
                    if scope == Scope::Global {
                        return Err("通用范围没有上级，请使用取消绑定".into());
                    }
                    if let Scope::App(a) = &scope
                        && let Some(i) = SIGNS.iter().position(|&s| s == sign)
                        && let Some(v) = p.applications.get_mut(a)
                    {
                        v[i] = None;
                    }
                    p.library
                        .rules
                        .retain(|r| r.scope != scope || r.sign != sign);
                } else {
                    let action = if id == 514 {
                        Action::Unassigned
                    } else if let Some(a) = ACTIONS.iter().find(|a| a.label() == value) {
                        *a
                    } else {
                        Action::Keys(value.parse()?)
                    };
                    p.set(scope, &sign, action);
                }
                Ok(())
            })?;
            refresh(w, &field(w, 510), &sign);
            set(w, 529, "绑定已加入草稿。");
        }
        518 | 519 => {
            let group = field(w, 516);
            if !library::valid_name(&group) {
                return Err("请输入应用组名称".into());
            }
            let members = field(w, 517)
                .split([',', '，', ';', '；', ' '])
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase)
                .collect();
            if id == 519 && !preview(w, "删除此组和组内绑定？", group.clone()) {
                return Ok(());
            }
            commit_edit(|p| {
                if id == 518 {
                    p.library.groups.insert(group.clone(), members);
                } else {
                    p.library.groups.remove(&group);
                    p.library
                        .rules
                        .retain(|r| r.scope != Scope::Group(group.clone()));
                }
                Ok(())
            })?;
            let scope = if id == 518 {
                format!("{GROUP}{group}")
            } else {
                GLOBAL.into()
            };
            refresh(w, &scope, &sign);
        }
        523 => {
            let Some(path) = file(w, false)? else {
                return Ok(());
            };
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024 {
                return Err("配置超过 8 MiB".into());
            }
            let bytes = fs::read(path).map_err(|e| e.to_string())?;
            let mut json: serde_json::Value =
                serde_json::from_slice(bytes.strip_prefix(&[239, 187, 191]).unwrap_or(&bytes))
                    .map_err(|e| e.to_string())?;
            let (next, summary, lines) = if json.get("glidequill_rules").is_some() {
                if json["glidequill_rules"] != 1 {
                    return Err("规则包版本不受支持".into());
                }
                let p: Pack =
                    serde_json::from_value(json["rules"].clone()).map_err(|e| e.to_string())?;
                p.validate()?;
                let detail = p.describe();
                (p, "用规则包替换当前规则草稿？".into(), detail)
            } else {
                if !select_scopes(w, &mut json)? {
                    return Ok(());
                }
                let imported = EDIT.with_borrow(|p| glidequill::migration::mouseinc(&json, p))?;
                (
                    imported.pack,
                    format!(
                        "可迁移 {} 条，停用或待支持 {} 条。确认后加入草稿。",
                        imported.active, imported.deferred
                    ),
                    imported.lines.join("\r\n"),
                )
            };
            if preview(w, &summary, lines) {
                EDIT.replace(next);
                refresh(w, GLOBAL, "LEFT");
                set(w, 529, "导入已加入草稿。返回后点击「保存并重载」生效。");
            }
        }
        524 => {
            if let Some(path) = file(w, true)? {
                EDIT.with_borrow(|p| export(&path, p))?;
                set(w, 529, "已导出当前规则草稿。");
            }
        }
        525 => {
            let p = settings::previous().map_err(|e| format!("无法读取上次保存版本：{e}"))?;
            if preview(w, "将上次保存前的规则载入草稿？", p.describe()) {
                EDIT.replace(p);
                refresh(w, GLOBAL, "LEFT");
            }
        }
        526 => {
            let lines = EDIT.with_borrow(|p| {
                p.library
                    .deferred
                    .iter()
                    .map(|v| {
                        format!(
                            "{} · {}　{}",
                            v.scope,
                            v.source["Name"].as_str().unwrap_or("未命名"),
                            v.reason
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\r\n")
            });
            preview(
                w,
                "保留在规则包中，桌面手势不会执行这些条目。",
                if lines.is_empty() {
                    "暂无待支持项".into()
                } else {
                    lines
                },
            );
        }
        527 => {
            DRAW.with_borrow_mut(Vec::clear);
            NAME.replace(
                if SIGNS.contains(&sign.as_str()) || library::WHEEL.contains(&sign.as_str()) {
                    String::new()
                } else {
                    sign.clone()
                },
            );
            if dlg(w, 402, Some(record)) == 1 {
                let name = NAME.with_borrow(Clone::clone);
                let points = DRAW.with_borrow(Clone::clone);
                commit_edit(|p| {
                    p.library.shapes.insert(name.clone(), points);
                    Ok(())
                })?;
                refresh(w, &field(w, 510), &name);
            }
        }
        528 => {
            if SIGNS.contains(&sign.as_str()) || library::WHEEL.contains(&sign.as_str()) {
                return Err("预设方向可取消绑定，不能删除".into());
            }
            if preview(w, "删除模板及其关联绑定？", sign.clone()) {
                commit_edit(|p| {
                    p.library.shapes.remove(&sign);
                    p.library.rules.retain(|r| r.sign != sign);
                    Ok(())
                })?;
                refresh(w, GLOBAL, "LEFT");
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn open(w: HWND) {
    EDIT.replace(rules::draft_pack());
    if dlg(w, 401, Some(manager)) == 1 {
        let pack = EDIT.with_borrow(Clone::clone);
        if !CONFIG_READABLE.get() {
            let detail = "原始文件会备份到本机恢复目录。使用当前规则草稿，恢复默认外观和时间，暂停全局手势。可读取的排除名单会保留。";
            if !preview(w, "从损坏配置恢复？", detail.into()) {
                return;
            }
            match settings::recover_rules(&pack) {
                Ok(dir) => {
                    CONFIG_READABLE.set(true);
                    enable_config_controls(w, true);
                    rules::replace_draft(pack);
                    label(
                        RESULT,
                        &format!("已恢复并暂停，原文件备份在 {}", dir.display()),
                    );
                }
                Err(e) => label(RESULT, &format!("恢复失败：{e}")),
            }
            return;
        }
        rules::replace_draft(pack);
        label(RESULT, "手势库草稿已更新，点击「保存并重载」应用。");
    }
}
unsafe extern "system" fn manager(w: HWND, m: u32, wp: usize, _: isize) -> isize {
    match m {
        WM_INITDIALOG => {
            refresh(w, GLOBAL, "LEFT");
            1
        }
        WM_CLOSE => {
            unsafe { EndDialog(w, 2) };
            1
        }
        WM_COMMAND => {
            let id = wp as u16 as i32;
            let code = (wp >> 16) as u32;
            if matches!(id, 510 | 511) && code == CBN_SELCHANGE {
                // Read the selection after the native combo has updated its edit control.
                unsafe { PostMessageW(w, WM_APP + 19, 0, 0) };
                return 1;
            }
            if code == 0 && matches!(id,1|2|513..=515|518|519|523..=528) {
                if let Err(e) = command(w, id) {
                    set(w, 529, &e);
                }
                return 1;
            }
            0
        }
        m if m == WM_APP + 19 => {
            show_binding(w);
            1
        }
        _ => 0,
    }
}

unsafe extern "system" fn canvas(
    w: HWND,
    m: u32,
    wp: usize,
    lp: isize,
    _: usize,
    _: usize,
) -> isize {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    unsafe {
        let point = [lp as i16 as i32, (lp >> 16) as i16 as i32];
        match m {
            WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                DRAW.replace(vec![point]);
                DRAWING.set(true);
                SetCapture(w);
                InvalidateRect(w, null_mut(), 0);
                0
            }
            WM_MOUSEMOVE if DRAWING.get() => {
                DRAW.with_borrow_mut(|p| library::append_sample(p, point));
                InvalidateRect(w, null_mut(), 0);
                0
            }
            WM_LBUTTONUP | WM_RBUTTONUP if DRAWING.replace(false) => {
                ReleaseCapture();
                DRAW.with_borrow_mut(|p| library::append_sample(p, point));
                InvalidateRect(w, null_mut(), 0);
                let name = DRAW.with_borrow(|points| {
                    EDIT.with_borrow(|p| {
                        let sig = library::signature(points)?;
                        let templates: Vec<_> = p
                            .library
                            .shapes
                            .iter()
                            .filter_map(|(n, pts)| Some((library::signature(pts)?, n)))
                            .collect();
                        let mut stroke = glidequill::Stroke::new(points[0][0], points[0][1]);
                        for &[x, y] in points {
                            stroke.sample(x, y);
                        }
                        match library::closest(
                            &sig,
                            &templates,
                            p.library.tuning.tolerance as f64 / 100.,
                        ) {
                            library::Match::Hit(name) => Some((*name).clone()),
                            library::Match::Ambiguous => Some("轨迹接近多个模板".into()),
                            library::Match::Miss => stroke.gesture().map(|g| g.name().to_owned()),
                        }
                    })
                });
                set(
                    GetParent(w),
                    533,
                    &format!("试画：{}", name.as_deref().unwrap_or("未识别")),
                );
                0
            }
            WM_CAPTURECHANGED => {
                if DRAWING.replace(false) {
                    DRAW.with_borrow_mut(Vec::clear);
                    InvalidateRect(w, null_mut(), 0);
                }
                0
            }
            WM_CONTEXTMENU => 0,
            _ => DefSubclassProc(w, m, wp, lp),
        }
    }
}
unsafe extern "system" fn record(w: HWND, m: u32, wp: usize, lp: isize) -> isize {
    unsafe {
        match m {
            WM_INITDIALOG => {
                RECORD_WINDOW.store(w, Ordering::Relaxed);
                DRAWING.set(false);
                SetWindowSubclass(GetDlgItem(w, 532), Some(canvas), 1, 0);
                set(w, 530, &NAME.with_borrow(Clone::clone));
                1
            }
            WM_DRAWITEM if wp == 532 => {
                let di = &*(lp as *const windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT);
                FillRect(di.hDC, &di.rcItem, GetSysColorBrush(COLOR_WINDOW));
                let pen = CreatePen(PS_SOLID, 2, 0xc46738);
                let old = SelectObject(di.hDC, pen as _);
                DRAW.with_borrow(|path| {
                    if let Some(([x, y], rest)) = path.split_first() {
                        MoveToEx(di.hDC, *x, *y, null_mut());
                        for &[x, y] in rest {
                            LineTo(di.hDC, x, y);
                        }
                    }
                });
                SelectObject(di.hDC, old);
                DeleteObject(pen as _);
                1
            }
            WM_COMMAND if wp as u16 == 1 => {
                let name = field(w, 530);
                if !library::valid_name(&name) || SIGNS.contains(&name.as_str()) {
                    set(w, 533, "请填写新的模板名称");
                    return 1;
                }
                if !DRAW.with_borrow(|p| {
                    library::signature(p).is_some()
                        && p.iter()
                            .any(|q| (q[0] - p[0][0]).abs().max((q[1] - p[0][1]).abs()) >= 16)
                }) {
                    set(w, 533, "请画出一笔完整轨迹");
                    return 1;
                }
                if EDIT.with_borrow(|p| p.library.shapes.contains_key(&name))
                    && !preview(w, "用新轨迹替换这个模板？", name.clone())
                {
                    return 1;
                }
                NAME.replace(name);
                EndDialog(w, 1);
                1
            }
            WM_CLOSE => {
                EndDialog(w, 2);
                1
            }
            WM_COMMAND if wp as u16 == 2 => {
                EndDialog(w, 2);
                1
            }
            WM_DESTROY => {
                RECORD_WINDOW.store(null_mut(), Ordering::Relaxed);
                DRAWING.set(false);
                0
            }
            _ => 0,
        }
    }
}

fn select_scopes(w: HWND, json: &mut serde_json::Value) -> Result<bool, String> {
    if !json["Gestures"].is_array() || !json["MatchGlobal"].is_array() {
        return Err("未找到 MouseInc 手势规则".into());
    }
    let mut items = vec![("通用规则".into(), true)];
    if let Some(groups) = json["MatchCustom"].as_array() {
        items.extend(
            groups
                .iter()
                .map(|g| (g["Name"].as_str().unwrap_or("未命名范围").to_owned(), true)),
        );
    }
    items.push(("全局热键（只归档）".into(), true));
    FILTER.replace(items);
    if dlg(w, 404, Some(filters)) != 1 {
        return Ok(false);
    }
    FILTER.with_borrow(|f| {
        if !f[0].1 {
            json["MatchGlobal"] = serde_json::json!([]);
        }
        if !f.last().unwrap().1 {
            json["Hotkeys"] = serde_json::json!([]);
        }
        if let Some(groups) = json
            .get_mut("MatchCustom")
            .and_then(serde_json::Value::as_array_mut)
        {
            let mut idx = 0;
            groups.retain(|_| {
                idx += 1;
                f[idx].1
            });
        }
    });
    Ok(true)
}
unsafe extern "system" fn filters(w: HWND, m: u32, wp: usize, _: isize) -> isize {
    match m {
        WM_INITDIALOG => {
            FILTER.with_borrow(|f| {
                for (i, (name, _)) in f.iter().enumerate() {
                    msg(w, 542, LB_ADDSTRING, 0, wide(name).as_ptr() as isize);
                    msg(w, 542, LB_SETSEL, 1, i as isize);
                }
            });
            1
        }
        WM_COMMAND if wp as u16 == 1 => {
            FILTER.with_borrow_mut(|f| {
                for (i, (_, on)) in f.iter_mut().enumerate() {
                    *on = msg(w, 542, LB_GETSEL, i, 0) > 0;
                }
            });
            unsafe { EndDialog(w, 1) };
            1
        }
        WM_CLOSE => {
            unsafe { EndDialog(w, 2) };
            1
        }
        WM_COMMAND if wp as u16 == 2 => {
            unsafe { EndDialog(w, 2) };
            1
        }
        _ => 0,
    }
}
