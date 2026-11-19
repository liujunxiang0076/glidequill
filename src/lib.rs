use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}
pub const DIRECTIONS: [Direction; 8] = [
    Direction::Left,
    Direction::Right,
    Direction::Up,
    Direction::Down,
    Direction::UpLeft,
    Direction::UpRight,
    Direction::DownLeft,
    Direction::DownRight,
];

impl Direction {
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
pub const DEFAULT_BINDINGS: [Action; 8] = [
    Action::Back,
    Action::Forward,
    Action::Home,
    Action::End,
    Action::Unassigned,
    Action::Unassigned,
    Action::Unassigned,
    Action::Unassigned,
];
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
        let displacement = x.hypot(y);
        if displacement < 24. || displacement < 0.78 * self.length {
            return None;
        }
        // Equal 45-degree sectors in screen coordinates (+Y is down).
        const TAN_HALF_SECTOR: f64 = std::f64::consts::SQRT_2 - 1.;
        Some(if y <= x * TAN_HALF_SECTOR {
            if self.dx < 0. {
                Direction::Left
            } else {
                Direction::Right
            }
        } else if x <= y * TAN_HALF_SECTOR {
            if self.dy < 0. {
                Direction::Up
            } else {
                Direction::Down
            }
        } else {
            match (self.dx < 0., self.dy < 0.) {
                (true, true) => Direction::UpLeft,
                (false, true) => Direction::UpRight,
                (true, false) => Direction::DownLeft,
                (false, false) => Direction::DownRight,
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eight_straight_directions_and_sector_boundaries() {
        use Direction::*;
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
            assert_eq!(stroke.direction(), Some(dir));
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
            assert_eq!(s.direction(), None);
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
}
