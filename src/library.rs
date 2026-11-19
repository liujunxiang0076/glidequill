use crate::{Action, DEFAULT_BINDINGS, GESTURES};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SIGNS: [&str; 20] = [
    "LEFT",
    "RIGHT",
    "UP",
    "DOWN",
    "\\ UP",
    "/ UP",
    "/ DOWN",
    "\\ DOWN",
    "UP-DOWN",
    "DOWN-UP",
    "LEFT-RIGHT",
    "RIGHT-LEFT",
    "UP-LEFT",
    "UP-RIGHT",
    "DOWN-LEFT",
    "DOWN-RIGHT",
    "LEFT-UP",
    "LEFT-DOWN",
    "RIGHT-UP",
    "RIGHT-DOWN",
];
pub type Bindings = [Action; GESTURES.len()];
pub type Overrides = [Option<Action>; GESTURES.len()];
pub type Applications = BTreeMap<String, Overrides>;
pub type Point = [i32; 2];
pub type Path = Vec<Point>;
pub const WHEEL: [&str; 2] = ["WheelSwitchUp", "WheelSwitchDown"];
pub const MAX_POINTS: usize = 512;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub tuning: Tuning,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_order: Vec<String>,
    #[serde(default = "stock_shapes")]
    pub shapes: BTreeMap<String, Path>,
    #[serde(default)]
    pub groups: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub deferred: Vec<Deferred>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tuning {
    pub distance: u16,
    pub tolerance: u16,
    pub trail: bool,
}
impl Default for Tuning {
    fn default() -> Self {
        Self {
            distance: 24,
            tolerance: 12,
            trail: true,
        }
    }
}
pub fn stock_shapes() -> BTreeMap<String, Path> {
    serde_json::from_str(include_str!("shapes.json")).expect("bundled gesture shapes")
}
impl Library {
    pub fn stock() -> Self {
        Self {
            shapes: stock_shapes(),
            ..Self::default()
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    App(String),
    Group(String),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub scope: Scope,
    pub sign: String,
    pub action: Action,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Deferred {
    pub scope: String,
    pub reason: String,
    pub source: serde_json::Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub bindings: Bindings,
    #[serde(default)]
    pub applications: Applications,
    #[serde(default = "Library::stock")]
    pub library: Library,
}
impl Default for Pack {
    fn default() -> Self {
        Self {
            bindings: DEFAULT_BINDINGS,
            applications: Applications::new(),
            library: Library::stock(),
        }
    }
}
pub fn valid_name(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
}
pub fn valid_app(s: &str) -> bool {
    valid_name(s)
        && s.ends_with(".exe")
        && !s.contains(['/', '\\', '*', '?', ':'])
        && s == s.to_lowercase()
}
impl Pack {
    pub fn set(&mut self, scope: Scope, sign: &str, action: Action) {
        if let Some(i) = SIGNS.iter().position(|&s| s == sign) {
            match &scope {
                Scope::Global => {
                    self.bindings[i] = action;
                    return;
                }
                Scope::App(name) => {
                    self.applications
                        .entry(name.clone())
                        .or_insert([None; GESTURES.len()])[i] = Some(action);
                    return;
                }
                _ => {}
            }
        }
        self.library
            .rules
            .retain(|r| r.scope != scope || r.sign != sign);
        self.library.rules.push(Rule {
            scope,
            sign: sign.into(),
            action,
        });
    }
    pub fn validate(&self) -> Result<(), String> {
        let lib = &self.library;
        if !(12..=96).contains(&lib.tuning.distance) || !(5..=22).contains(&lib.tuning.tolerance) {
            return Err("触发距离或模板容差超出范围".into());
        }
        if lib.template_order.len() > 512 || lib.template_order.iter().any(|s| !valid_name(s)) {
            return Err("模板目录无效".into());
        }
        if lib.shapes.len() > 256
            || lib.groups.len() > 128
            || self.applications.len() > 512
            || lib.rules.len() > 4096
            || lib.deferred.len() > 4096
        {
            return Err("规则或模板数量超出支持范围".into());
        }
        for (name, points) in &lib.shapes {
            if !valid_name(name)
                || SIGNS.contains(&name.as_str())
                || WHEEL.contains(&name.as_str())
                || !(2..=MAX_POINTS).contains(&points.len())
                || points
                    .iter()
                    .flatten()
                    .any(|v| v.unsigned_abs() > 1_000_000)
                || signature(points).is_none()
            {
                return Err(format!("无效轨迹：{name}"));
            }
        }
        if self.applications.keys().any(|a| !valid_app(a)) {
            return Err("无效的程序文件名".into());
        }
        for (name, members) in &lib.groups {
            if !valid_name(name)
                || members.is_empty()
                || members.len() > 256
                || members.iter().any(|a| !valid_app(a))
                || members.iter().collect::<BTreeSet<_>>().len() != members.len()
            {
                return Err(format!("应用组无效：{name}"));
            }
        }
        let mut pairs = BTreeSet::new();
        let mut grouped = BTreeSet::new();
        for r in &lib.rules {
            let basic = SIGNS.contains(&r.sign.as_str());
            if WHEEL.contains(&r.sign.as_str())
                && r.action != Action::Unassigned
                && r.action.keys().is_none()
            {
                return Err("滚轮快切仅接受按键动作".into());
            }
            if (!basic && !WHEEL.contains(&r.sign.as_str()) && !lib.shapes.contains_key(&r.sign))
                || !pairs.insert((&r.scope, &r.sign))
            {
                return Err(format!("未知或重复的手势：{}", r.sign));
            }
            match &r.scope {
                Scope::Group(g) => {
                    for exe in lib.groups.get(g).ok_or("找不到应用组")? {
                        if !grouped.insert((exe, &r.sign)) {
                            return Err(format!("{exe} 的 {} 有多个组绑定", r.sign));
                        }
                    }
                }
                Scope::App(a) if !valid_app(a) || basic => return Err("无效的应用规则".into()),
                Scope::Global if basic => return Err("通用方向重复".into()),
                _ => {}
            }
        }
        Ok(())
    }
}
impl Library {
    pub fn action(&self, exe: &str, sign: &str) -> Option<Action> {
        self.rules
            .iter()
            .filter(|r| r.sign == sign)
            .filter_map(|r| {
                let rank = match &r.scope {
                    Scope::Global => 0,
                    Scope::Group(g)
                        if self
                            .groups
                            .get(g)
                            .is_some_and(|v| v.iter().any(|a| a.eq_ignore_ascii_case(exe))) =>
                    {
                        1
                    }
                    Scope::App(a) if a.eq_ignore_ascii_case(exe) => 2,
                    _ => return None,
                };
                Some((rank, r.action))
            })
            .max_by_key(|p| p.0)
            .map(|p| p.1)
    }
}
pub fn append_sample(points: &mut Path, p: Point) {
    if points.last() == Some(&p) {
        return;
    }
    if points.len() == MAX_POINTS {
        for i in 1..MAX_POINTS / 2 {
            points[i] = points[i * 2]
        }
        points.truncate(MAX_POINTS / 2);
    }
    points.push(p);
}
const N: usize = 32;
pub type Signature = [[f64; 2]; N];
// Resample at equal arc lengths, preserving orientation, sequence and aspect ratio.
pub fn signature(points: &[Point]) -> Option<Signature> {
    let first = points.first()?;
    let mut min = *first;
    let mut max = *first;
    let length = |a: Point, b: Point| (a[0] as f64 - b[0] as f64).hypot(a[1] as f64 - b[1] as f64);
    let mut total = 0.;
    for pair in points.windows(2) {
        total += length(pair[0], pair[1]);
        for i in 0..2 {
            min[i] = min[i].min(pair[1][i]);
            max[i] = max[i].max(pair[1][i]);
        }
    }
    let scale = (max[0] as f64 - min[0] as f64).max(max[1] as f64 - min[1] as f64);
    if scale < 1. || total < 1. {
        return None;
    }
    let mut sample = [[0.; 2]; N];
    let mut j = 1;
    let mut traversed = 0.;
    for (i, p) in sample.iter_mut().enumerate() {
        let wanted = i as f64 * total / (N - 1) as f64;
        while j + 1 < points.len() && traversed + length(points[j - 1], points[j]) < wanted {
            traversed += length(points[j - 1], points[j]);
            j += 1;
        }
        let a = points[j - 1];
        let b = points[j];
        let t = ((wanted - traversed) / length(a, b).max(1e-9)).clamp(0., 1.);
        for k in 0..2 {
            p[k] = (a[k] as f64 + (b[k] as f64 - a[k] as f64) * t
                - (min[k] as f64 + max[k] as f64) / 2.)
                / scale;
        }
    }
    Some(sample)
}
#[derive(Debug, PartialEq)]
pub enum Match<T> {
    Hit(T),
    Ambiguous,
    Miss,
}
pub fn closest<'a, T>(
    sample: &Signature,
    templates: &'a [(Signature, T)],
    limit: f64,
) -> Match<&'a T> {
    let mut best = (f64::INFINITY, None);
    let mut next = f64::INFINITY;
    for (shape, id) in templates {
        let error = shape
            .iter()
            .zip(sample)
            .map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]))
            .sum::<f64>()
            / N as f64;
        if error < best.0 {
            next = best.0;
            best = (error, Some(id));
        } else {
            next = next.min(error);
        }
    }
    match best {
        (distance, Some(id)) if distance <= limit => {
            if next - distance > 0.025 {
                Match::Hit(id)
            } else {
                Match::Ambiguous
            }
        }
        _ => Match::Miss,
    }
}

impl Pack {
    pub fn describe(&self) -> String {
        let mut lines = vec![format!(
            "方向 {} · 图形 {} · 应用组 {}",
            SIGNS.len(),
            self.library.shapes.len(),
            self.library.groups.len()
        )];
        for (s, a) in SIGNS.iter().zip(self.bindings) {
            lines.push(format!("通用 · {s} → {}", a.display()));
        }
        for (name, bindings) in &self.applications {
            for (s, a) in SIGNS.iter().zip(bindings) {
                if let Some(a) = a {
                    lines.push(format!("{name} · {s} → {}", a.display()));
                }
            }
        }
        for (g, members) in &self.library.groups {
            lines.push(format!("应用组 {g}：{}", members.join(", ")));
        }
        for r in &self.library.rules {
            let scope = match &r.scope {
                Scope::Global => "通用",
                Scope::App(a) | Scope::Group(a) => a,
            };
            lines.push(format!("{scope} · {} → {}", r.sign, r.action.display()));
        }
        lines.push(format!("停用或待支持 {} 项", self.library.deferred.len()));
        lines.join("\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drawn_shapes_survive_translation_scaling_and_resampling() {
        let shapes = stock_shapes();
        assert_eq!(shapes.len(), 16);
        let templates: Vec<_> = shapes
            .iter()
            .map(|(n, p)| (signature(p).unwrap(), n))
            .collect();
        for (n, pts) in &shapes {
            let mut path = vec![];
            for pair in pts.windows(2) {
                let [a, b] = [pair[0], pair[1]];
                for t in 0..8 {
                    path.push([
                        200 + a[0] * 2 + (b[0] - a[0]) * t / 4,
                        90 + a[1] * 2 + (b[1] - a[1]) * t / 4 + (t % 2),
                    ]);
                }
            }
            let e = pts.last().unwrap();
            path.push([200 + 2 * e[0], 90 + 2 * e[1]]);
            assert_eq!(
                closest(&signature(&path).unwrap(), &templates, 0.12),
                Match::Hit(&n),
                "{n}"
            );
        }
    }
    #[test]
    fn ambiguous_shapes_and_conflicting_groups_do_not_dispatch() {
        let shape = signature(&[[0, 0], [0, 9], [9, 9]]).unwrap();
        assert_eq!(
            closest(&shape, &[(shape, 0), (shape, 1)], 0.12),
            Match::Ambiguous
        );
        let mut p = Pack::default();
        for group in ["A", "B"] {
            p.library
                .groups
                .insert(group.into(), vec!["edge.exe".into()]);
            p.set(Scope::Group(group.into()), "LEFT", Action::Forward);
        }
        assert!(p.validate().is_err());
    }
}
