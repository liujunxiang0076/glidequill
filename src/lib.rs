use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

pub const DIRECTIONS: [Direction; 4] = [
    Direction::Left,
    Direction::Right,
    Direction::Up,
    Direction::Down,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Inherit,
    Off,
    Back,
    Forward,
    Copy,
    Paste,
    Home,
    End,
}
pub const ACTIONS: [(Action, &str); 8] = [
    (Action::Inherit, "沿用通用"),
    (Action::Off, "停用"),
    (Action::Back, "后退"),
    (Action::Forward, "前进"),
    (Action::Copy, "复制"),
    (Action::Paste, "粘贴"),
    (Action::Home, "页面顶部"),
    (Action::End, "页面底部"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub global: [Action; 4],
    pub browser: [Action; 4],
    #[serde(default)]
    pub excluded: Vec<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            global: [Action::Back, Action::Forward, Action::Off, Action::Off],
            browser: [Action::Inherit; 4],
            excluded: vec![],
        }
    }
}
impl Config {
    pub fn resolve(&self, browser: bool, dir: Direction) -> Action {
        let i = dir as usize;
        if browser && self.browser[i] != Action::Inherit {
            self.browser[i]
        } else {
            self.global[i]
        }
    }
}
pub fn browser(exe: &str) -> bool {
    ["chrome.exe", "msedge.exe", "firefox.exe"]
        .iter()
        .any(|s| exe.eq_ignore_ascii_case(s))
}
/// Track distance as well as displacement so loops cannot turn into accidental swipes.
#[derive(Default)]
pub struct Stroke {
    last: (i32, i32),
    dx: f64,
    dy: f64,
    length: f64,
    far: f64,
}
impl Stroke {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            last: (x, y),
            ..Self::default()
        }
    }
    pub fn sample(&mut self, x: i32, y: i32) {
        let dx = f64::from(x) - f64::from(self.last.0);
        let dy = f64::from(y) - f64::from(self.last.1);
        self.dx += dx;
        self.dy += dy;
        self.length += dx.hypot(dy);
        self.far = self.far.max(self.dx.hypot(self.dy));
        self.last = (x, y);
    }
    pub fn is_click(&self) -> bool {
        self.far < 8.0
    }
    pub fn direction(&self) -> Option<Direction> {
        let x = self.dx.abs();
        let y = self.dy.abs();
        if x.max(y) < 24.0 || x.min(y) > 0.55 * x.max(y) || x.hypot(y) < 0.78 * self.length {
            return None;
        }
        Some(if x > y {
            if self.dx > 0.0 {
                Direction::Right
            } else {
                Direction::Left
            }
        } else if self.dy > 0.0 {
            Direction::Down
        } else {
            Direction::Up
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_jitter_diagonal_and_loops() {
        for pts in [vec![(3, 2)], vec![(60, 60)], vec![(80, 0), (0, 0), (30, 0)]] {
            let mut s = Stroke::new(0, 0);
            for (x, y) in pts {
                s.sample(x, y)
            }
            assert_eq!(s.direction(), None)
        }
    }
    #[test]
    fn swipe_on_negative_monitor() {
        let mut s = Stroke::new(-200, 400);
        s.sample(-260, 402);
        assert_eq!(s.direction(), Some(Direction::Left));
        assert!(!s.is_click())
    }
    #[test]
    fn returning_to_origin_is_not_a_click() {
        let mut s = Stroke::new(0, 0);
        s.sample(70, 0);
        s.sample(0, 0);
        assert!(!s.is_click());
    }
    #[test]
    fn explicit_off_overrides_global() {
        let mut c = Config::default();
        assert_eq!(c.resolve(true, Direction::Left), Action::Back);
        c.browser[0] = Action::Off;
        assert_eq!(c.resolve(true, Direction::Left), Action::Off);
        assert_eq!(c.resolve(false, Direction::Left), Action::Back)
    }
    #[test]
    fn config_roundtrip() {
        let c: Config =
            serde_json::from_str(&serde_json::to_string(&Config::default()).unwrap()).unwrap();
        assert!(browser("CHROME.EXE"));
        assert_eq!(c.global, Config::default().global);
    }
}
