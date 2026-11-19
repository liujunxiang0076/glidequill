use crate::library::*;
use crate::{ACTIONS, Action};
use serde_json::Value;
use std::collections::BTreeSet;

pub struct Import {
    pub pack: Pack,
    pub lines: Vec<String>,
    pub active: usize,
    pub deferred: usize,
}
fn list<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], String> {
    v.get(key).map_or(Ok(&[][..]), |a| {
        a.as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("{key} 需要列表"))
    })
}
fn action(row: &Value) -> Option<Action> {
    let chain = row.get("Actions")?.as_array()?;
    let [step] = chain.as_slice() else {
        return None;
    };
    let [cmd, param] = step.as_array()?.as_slice() else {
        return None;
    };
    let (cmd, param) = (cmd.as_str()?, param.as_str()?);
    if cmd.eq_ignore_ascii_case("SendKeys") {
        let keys = param.parse().ok()?;
        Some(
            ACTIONS
                .iter()
                .copied()
                .find(|a| a.keys() == Some(keys))
                .unwrap_or(Action::Keys(keys)),
        )
    } else if cmd.eq_ignore_ascii_case("Window") {
        match param.to_ascii_lowercase().as_str() {
            "maximize" => Some(Action::Maximize),
            "minimize" => Some(Action::Minimize),
            "close" => Some(Action::Close),
            _ => None,
        }
    } else {
        None
    }
}
impl Import {
    fn defer(&mut self, scope: &str, row: &Value, reason: &str) {
        let item = Deferred {
            scope: scope.into(),
            reason: reason.into(),
            source: row.clone(),
        };
        if !self.pack.library.deferred.contains(&item) {
            self.pack.library.deferred.push(item);
        }
        self.deferred += 1;
        self.lines.push(format!(
            "{scope} · {}　{reason}",
            row["Sign"]
                .as_str()
                .or(row["Name"].as_str())
                .unwrap_or("条目")
        ));
    }
    fn rows(&mut self, scope: Scope, name: &str, rows: &[Value]) {
        let mut seen = BTreeSet::new();
        for row in rows {
            let sign = row["Sign"].as_str().unwrap_or("");
            let reason = if row["Valid"].as_bool() != Some(true) {
                "原已停用或状态未知"
            } else if !SIGNS.contains(&sign)
                && !WHEEL.contains(&sign)
                && !self.pack.library.shapes.contains_key(sign)
            {
                "待支持的触发方式"
            } else if seen.contains(sign) {
                "同范围重复，需审阅"
            } else if let Some(action) =
                action(row).filter(|a| !WHEEL.contains(&sign) || a.keys().is_some())
            {
                self.pack.set(scope.clone(), sign, action);
                seen.insert(sign);
                self.lines
                    .push(format!("{name} · {sign} → {}", action.display()));
                self.active += 1;
                continue;
            } else {
                seen.insert(sign);
                self.pack.set(scope.clone(), sign, Action::Unassigned);
                "待支持的动作或动作链"
            };
            self.defer(name, row, reason);
        }
    }
}
pub fn mouseinc(source: &Value, base: &Pack) -> Result<Import, String> {
    if !source["Gestures"].is_array() || !source["MatchGlobal"].is_array() {
        return Err("不是 MouseInc 手势配置".into());
    }
    let mut out = Import {
        pack: base.clone(),
        lines: vec![],
        active: 0,
        deferred: 0,
    };
    let mut order = Vec::new();
    for shape in list(source, "Gestures")? {
        let name = shape["Sign"].as_str().ok_or("模板缺少名称")?;
        if order.iter().any(|n| n == name) {
            return Err(format!("模板名称重复：{name}"));
        }
        order.push(name.to_owned());
        let data = shape["Data"].as_array().ok_or("模板缺少坐标")?;
        if data.len() % 2 != 0 || data.len() > MAX_POINTS * 2 {
            return Err(format!("模板坐标无效：{name}"));
        }
        let mut points = vec![];
        for p in data.chunks_exact(2) {
            let n = |v: &Value| {
                v.as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .filter(|v| v.unsigned_abs() <= 1_000_000)
                    .ok_or("坐标必须是支持范围内的整数")
            };
            points.push([n(&p[0])?, n(&p[1])?]);
        }
        if signature(&points).is_none() {
            return Err(format!("无效模板：{name}"));
        }
        if let Some(i) = SIGNS.iter().position(|&s| s == name) {
            let mut stroke = crate::Stroke::new(points[0][0], points[0][1]);
            for &[x, y] in &points {
                stroke.sample(x, y);
            }
            if stroke.gesture() != Some(crate::GESTURES[i]) {
                return Err(format!(
                    "内置名称 {name} 与文件中的画法不符，请先为自定义形状另命名"
                ));
            }
        } else {
            out.pack.library.shapes.insert(name.into(), points);
        }
        out.lines.push(format!("模板 · {name}"));
    }
    out.pack.library.template_order = order;
    out.rows(Scope::Global, "通用", list(source, "MatchGlobal")?);
    for group in list(source, "MatchCustom")? {
        let name = group["Name"].as_str().ok_or("应用组缺少名称")?;
        let members: Vec<_> = list(group, "Match")?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_lowercase)
            .collect();
        if members.is_empty()
            || members.len() != list(group, "Match")?.len()
            || members.iter().any(|a| !valid_app(a))
            || group["IgnoreGlobal"] != false
        {
            out.defer(name, group, "特殊匹配条件，需审阅");
            continue;
        }
        let scope = if members.len() == 1 {
            Scope::App(members[0].clone())
        } else {
            if out
                .pack
                .library
                .groups
                .get(name)
                .is_some_and(|v| v != &members)
            {
                out.defer(name, group, "同名应用组成员不同，需审阅");
                continue;
            }
            out.pack.library.groups.insert(name.into(), members);
            Scope::Group(name.into())
        };
        out.rows(scope, name, list(group, "List")?);
    }
    for hotkey in list(source, "Hotkeys")? {
        out.defer(
            "全局热键",
            hotkey,
            if hotkey["Valid"] == false {
                "原已停用"
            } else {
                "待支持的全局热键"
            },
        );
    }
    out.pack.validate()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn import_is_staged_and_keeps_chains_inert() {
        let source = json!({"Gestures":[{"Sign":"Z","Data":[0,0,100,0,0,100,100,100]}],
    "MatchGlobal":[{"Sign":"LEFT","Valid":true,"Actions":[["SendKeys","Ctrl+C"],["Window","Close"]]},
        {"Sign":"Z","Valid":true,"Actions":[["SendKeys","Ctrl+Alt+F12"]]},
        {"Sign":"Z","Valid":false,"Actions":[["Window","Close"]]}],
    "MatchCustom":[{"Name":"浏览器","IgnoreGlobal":false,"Match":["chrome.exe","msedge.exe"],"List":[{"Sign":"UP","Valid":true,"Actions":[["SendKeys","Ctrl+Shift+R"]]}]}]});
        let base = Pack::default();
        let r = mouseinc(&source, &base).unwrap();
        assert_eq!((r.active, r.deferred), (2, 2));
        assert_eq!(base.bindings[0], Action::Back);
        assert_eq!(r.pack.bindings[0], Action::Unassigned);
        assert_eq!(
            r.pack.library.action("chrome.exe", "UP"),
            Some(Action::HardRefresh)
        );
        assert_eq!(
            r.pack.library.action("test.exe", "Z").unwrap().display(),
            "Ctrl+Alt+F12"
        );
        let again = mouseinc(&source, &r.pack).unwrap();
        assert_eq!(again.pack, r.pack);
        let saved = serde_json::to_vec(&r.pack).unwrap();
        assert_eq!(serde_json::from_slice::<Pack>(&saved).unwrap(), r.pack);
    }
    #[test]
    #[ignore = "set GLIDEQUILL_MOUSEINC_FIXTURE to a local MouseInc 2.13.4 configuration"]
    fn installed_mouseinc_migration() {
        let path = std::env::var_os("GLIDEQUILL_MOUSEINC_FIXTURE").unwrap();
        let source = std::fs::read(path).unwrap();
        let v = serde_json::from_slice(source.strip_prefix(&[239, 187, 191]).unwrap_or(&source))
            .unwrap();
        let imported = mouseinc(&v, &Pack::default()).unwrap();
        assert_eq!(imported.pack.library.template_order.len(), 36);
        assert_eq!(imported.active + imported.deferred, 47);
        assert_eq!(
            imported.pack.library.action("msedge.exe", "DOWN-UP"),
            Some(Action::Refresh)
        );
        assert_eq!(imported.pack.bindings[6], Action::Minimize);
        assert_eq!(
            imported
                .pack
                .library
                .action("devenv.exe", "WheelSwitchUp")
                .unwrap()
                .display(),
            "Ctrl+Alt+PgUp"
        );
        println!(
            "36 templates, {} executable rules, {} archived rules/hotkeys",
            imported.active, imported.deferred
        );
    }
}
