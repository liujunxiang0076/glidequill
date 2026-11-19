use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Gesture {
    Left,
    Right,
    Up,
    Down,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
    UpDown,
    DownUp,
    LeftRight,
    RightLeft,
    UpThenLeft,
    UpThenRight,
    DownThenLeft,
    DownThenRight,
}
pub const BASIC_COUNT: usize = 8;
pub const GESTURES: [Gesture; 16] = [
    Gesture::Left,
    Gesture::Right,
    Gesture::Up,
    Gesture::Down,
    Gesture::UpLeft,
    Gesture::UpRight,
    Gesture::DownLeft,
    Gesture::DownRight,
    Gesture::UpDown,
    Gesture::DownUp,
    Gesture::LeftRight,
    Gesture::RightLeft,
    Gesture::UpThenLeft,
    Gesture::UpThenRight,
    Gesture::DownThenLeft,
    Gesture::DownThenRight,
];

impl Gesture {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "向左",
            Self::Right => "向右",
            Self::Up => "向上",
            Self::Down => "向下",
            Self::UpLeft => "左上",
            Self::UpRight => "右上",
            Self::DownLeft => "左下",
            Self::DownRight => "右下",
            Self::UpDown => "上再下",
            Self::DownUp => "下再上",
            Self::LeftRight => "左再右",
            Self::RightLeft => "右再左",
            Self::UpThenLeft => "上再左",
            Self::UpThenRight => "上再右",
            Self::DownThenLeft => "下再左",
            Self::DownThenRight => "下再右",
        }
    }
    pub const fn arrow(self) -> &'static str {
        match self {
            Self::Left => "←",
            Self::Right => "→",
            Self::Up => "↑",
            Self::Down => "↓",
            Self::UpLeft => "↖",
            Self::UpRight => "↗",
            Self::DownLeft => "↙",
            Self::DownRight => "↘",
            Self::UpDown => "↑↓",
            Self::DownUp => "↓↑",
            Self::LeftRight => "←→",
            Self::RightLeft => "→←",
            Self::UpThenLeft => "↑←",
            Self::UpThenRight => "↑→",
            Self::DownThenLeft => "↓←",
            Self::DownThenRight => "↓→",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum Action {
    Unassigned,
    Back,
    Forward,
    Home,
    End,
    Copy,
    Paste,
    Minimize,
    Maximize,
    Close,
}
pub const ACTIONS: [Action; 10] = [
    Action::Unassigned,
    Action::Back,
    Action::Forward,
    Action::Home,
    Action::End,
    Action::Copy,
    Action::Paste,
    Action::Minimize,
    Action::Maximize,
    Action::Close,
];
pub const DEFAULT_BINDINGS: [Action; GESTURES.len()] = {
    let mut bindings = [Action::Unassigned; GESTURES.len()];
    bindings[Gesture::Left as usize] = Action::Back;
    bindings[Gesture::Right as usize] = Action::Forward;
    bindings[Gesture::Up as usize] = Action::Home;
    bindings[Gesture::Down as usize] = Action::End;
    bindings
};
impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unassigned => "未绑定",
            Self::Back => "后退",
            Self::Forward => "前进",
            Self::Home => "页面顶部",
            Self::End => "页面底部",
            Self::Copy => "复制",
            Self::Paste => "粘贴",
            Self::Minimize => "最小化窗口",
            Self::Maximize => "最大化／还原",
            Self::Close => "关闭窗口",
        }
    }
}
type Axis = (f64, f64);
const UP: Axis = (0., -1.);
const DOWN: Axis = (0., 1.);
const LEFT: Axis = (-1., 0.);
const RIGHT: Axis = (1., 0.);
const TWO_LEG: [(Gesture, Axis, Axis); 8] = [
    (Gesture::UpDown, UP, DOWN),
    (Gesture::DownUp, DOWN, UP),
    (Gesture::LeftRight, LEFT, RIGHT),
    (Gesture::RightLeft, RIGHT, LEFT),
    (Gesture::UpThenLeft, UP, LEFT),
    (Gesture::UpThenRight, UP, RIGHT),
    (Gesture::DownThenLeft, DOWN, LEFT),
    (Gesture::DownThenRight, DOWN, RIGHT),
];
#[derive(Default, Clone, Copy)]
struct Pivot {
    position: Axis,
    length: f64,
    score: f64,
}

fn follows_axis((x, y): Axis, length: f64, (ax, ay): Axis) -> bool {
    let forward = x * ax + y * ay;
    forward >= 24.0 && forward >= 0.9 * length && (x * ay - y * ax).abs() <= 0.36 * forward
}

/// Track distance as well as displacement so loops cannot turn into accidental swipes.
#[derive(Default)]
pub struct Stroke {
    last: (i32, i32),
    dx: f64,
    dy: f64,
    length: f64,
    far: f64,
    pivots: [Pivot; TWO_LEG.len()],
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
        // Projection onto the incoming axis minus the outgoing axis peaks at the turn.
        // Keep that point and its path length, independent of stroke duration.
        for (pivot, (_, a, b)) in self.pivots.iter_mut().zip(TWO_LEG) {
            let score = self.dx * (a.0 - b.0) + self.dy * (a.1 - b.1);
            if score > pivot.score {
                *pivot = Pivot {
                    position: (self.dx, self.dy),
                    length: self.length,
                    score,
                };
            }
        }
        self.last = (x, y);
    }
    pub fn is_click(&self) -> bool {
        self.far < 8.0
    }
    pub fn gesture(&self) -> Option<Gesture> {
        for (pivot, (gesture, a, b)) in self.pivots.iter().zip(TWO_LEG) {
            let tail = (self.dx - pivot.position.0, self.dy - pivot.position.1);
            if follows_axis(pivot.position, pivot.length, a)
                && follows_axis(tail, self.length - pivot.length, b)
            {
                return Some(gesture);
            }
        }
        self.direction()
    }
    fn direction(&self) -> Option<Gesture> {
        let x = self.dx.abs();
        let y = self.dy.abs();
        let displacement = x.hypot(y);
        if displacement < 24. || displacement < 0.78 * self.length {
            return None;
        }
        // Equal 45-degree sectors in screen coordinates (+Y is down).
        const TAN_HALF_SECTOR: f64 = std::f64::consts::SQRT_2 - 1.;
        Some(if y <= x * TAN_HALF_SECTOR {
            if self.dx < 0. {
                Gesture::Left
            } else {
                Gesture::Right
            }
        } else if x <= y * TAN_HALF_SECTOR {
            if self.dy < 0. {
                Gesture::Up
            } else {
                Gesture::Down
            }
        } else {
            match (self.dx < 0., self.dy < 0.) {
                (true, true) => Gesture::UpLeft,
                (false, true) => Gesture::UpRight,
                (true, false) => Gesture::DownLeft,
                (false, false) => Gesture::DownRight,
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eight_straight_directions_and_sector_boundaries() {
        use Gesture::*;
        for (x, y, dir) in [
            (-60, 0, Left),
            (60, 0, Right),
            (0, -60, Up),
            (0, 60, Down),
            (-50, -50, UpLeft),
            (50, -50, UpRight),
            (-50, 50, DownLeft),
            (50, 50, DownRight),
            (100, 41, Right),
            (100, 42, DownRight),
            (41, 100, Down),
            (42, 100, DownRight),
        ] {
            let mut stroke = Stroke::new(0, 0);
            stroke.sample(x, y);
            assert_eq!(stroke.gesture(), Some(dir));
        }
    }
    #[test]
    fn reject_jitter_bends_and_loops() {
        for points in [
            vec![(3, 2)],
            vec![(16, 16)],
            vec![(60, 0), (60, 60)],
            vec![(80, 0), (0, 0), (30, 0)],
        ] {
            let mut s = Stroke::new(0, 0);
            for (x, y) in points {
                s.sample(x, y);
            }
            assert_eq!(s.gesture(), None);
        }
    }
    #[test]
    fn two_leg_strokes_tolerate_drift_and_do_not_match_a_short_return() {
        for (expected, a, b) in TWO_LEG {
            let mut s = Stroke::new(0, 0);
            let corner = (a.0 * 88.0, a.1 * 88.0);
            for n in 1..=11 {
                let step = f64::from(n * 8);
                s.sample((a.0 * step) as i32 + n % 3, (a.1 * step) as i32);
            }
            s.sample((corner.0 + b.0 * 8.) as i32, (corner.1 + b.1 * 8.) as i32);
            assert_ne!(s.gesture(), Some(expected));
            for n in 2..=7 {
                let step = f64::from(n * 8);
                s.sample(
                    (corner.0 + b.0 * step) as i32,
                    (corner.1 + b.1 * step) as i32 + n % 3,
                );
            }
            assert_eq!(s.gesture(), Some(expected));
        }
    }
    #[test]
    fn swipe_on_negative_monitor() {
        let mut s = Stroke::new(-200, 400);
        s.sample(-260, 402);
        assert_eq!(s.gesture(), Some(Gesture::Left));
        assert!(!s.is_click())
    }
    #[test]
    fn returning_to_origin_is_not_a_click() {
        let mut s = Stroke::new(0, 0);
        s.sample(70, 0);
        s.sample(0, 0);
        assert!(!s.is_click());
    }
}
