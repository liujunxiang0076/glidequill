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
    pub fn legs(self) -> Option<((f64, f64), (f64, f64))> {
        TWO_LEG
            .iter()
            .find(|(gesture, _, _)| *gesture == self)
            .map(|&(_, a, b)| (a, b))
    }
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
struct Travel {
    length: f64,
    abs_x: f64,
    abs_y: f64,
}
impl Travel {
    fn advance(self, dx: f64, dy: f64, distance: f64) -> Self {
        Self {
            length: self.length + distance,
            abs_x: self.abs_x + dx.abs(),
            abs_y: self.abs_y + dy.abs(),
        }
    }
    fn since(self, earlier: Self) -> Self {
        Self {
            length: self.length - earlier.length,
            abs_x: self.abs_x - earlier.abs_x,
            abs_y: self.abs_y - earlier.abs_y,
        }
    }
}
#[derive(Default, Clone, Copy)]
struct Pivot {
    position: Axis,
    travel: Travel,
    projection: f64,
}

fn leg_score((x, y): Axis, travel: Travel, (ax, ay): Axis, minimum: f64) -> Option<f64> {
    let forward = x * ax + y * ay;
    let sideways = (x * ay - y * ax).abs();
    let axial_distance = travel.abs_x * ax.abs() + travel.abs_y * ay.abs();
    let backtrack = (axial_distance - forward) * 0.5;
    // Allow a rounded bend and lateral drift, but require sustained travel in order.
    if forward < minimum
        || sideways > forward * 0.80
        || forward < travel.length * 0.69
        || backtrack > 4.0 + 0.12 * forward
    {
        return None;
    }
    Some(forward / travel.length)
}

#[derive(Default)]
pub struct Stroke {
    origin: (i32, i32),
    dx: f64,
    dy: f64,
    anchor: Axis,
    anchored_travel: Travel,
    travel: Travel,
    far: f64,
    pivots: [Pivot; TWO_LEG.len()],
}
impl Stroke {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            origin: (x, y),
            ..Self::default()
        }
    }
    pub fn sample(&mut self, x: i32, y: i32) {
        self.dx = f64::from(x) - f64::from(self.origin.0);
        self.dy = f64::from(y) - f64::from(self.origin.1);
        self.far = self.far.max(self.dx.hypot(self.dy));
        let dx = self.dx - self.anchor.0;
        let dy = self.dy - self.anchor.1;
        let distance = dx.hypot(dy);
        self.travel = self.anchored_travel.advance(dx, dy, distance);
        // Count motion spatially, so dense 1–2 DIP jitter does not inflate path length.
        // The current endpoint still participates in live preview and on release.
        if distance >= 4.0 {
            self.anchor = (self.dx, self.dy);
            self.anchored_travel = self.travel;
        }
        for (pivot, (_, a, b)) in self.pivots.iter_mut().zip(TWO_LEG) {
            let projection = self.dx * (a.0 - b.0) + self.dy * (a.1 - b.1);
            if projection > pivot.projection {
                *pivot = Pivot {
                    position: (self.dx, self.dy),
                    travel: self.travel,
                    projection,
                };
            }
        }
    }
    pub fn is_click(&self) -> bool {
        self.far < 8.0
    }
    pub fn gesture(&self) -> Option<Gesture> {
        let mut best = None;
        let mut best_score = 0.0;
        for (pivot, (gesture, a, b)) in self.pivots.iter().zip(TWO_LEG) {
            let first = pivot.position;
            let last = (self.dx - first.0, self.dy - first.1);
            let Some(enter) = leg_score(first, pivot.travel, a, 24.0) else {
                continue;
            };
            let min_exit = (0.18 * (first.0 * a.0 + first.1 * a.1)).clamp(18.0, 32.0);
            let Some(exit) = leg_score(last, self.travel.since(pivot.travel), b, min_exit) else {
                continue;
            };
            let cosine = (first.0 * last.0 + first.1 * last.1)
                / (first.0.hypot(first.1) * last.0.hypot(last.1));
            let is_return = a.0 == -b.0 && a.1 == -b.1;
            if (is_return && cosine > -0.45) || (!is_return && cosine.abs() > 0.62) {
                continue;
            }
            let score = enter + exit;
            if score > best_score {
                best = Some(gesture);
                best_score = score;
            }
        }
        best.or_else(|| self.direction())
    }
    fn direction(&self) -> Option<Gesture> {
        let x = self.dx.abs();
        let y = self.dy.abs();
        let displacement = x.hypot(y);
        if displacement < 24. || displacement < 0.78 * self.travel.length {
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
    fn rounded_continuous_strokes_at_different_sampling_densities() {
        // Relative coordinates along/across the incoming axis. The hand rolls
        // through the bend and ends off the first leg, often well short of its start.
        let returns = [
            (0, 0),
            (16, 5),
            (34, 10),
            (49, 17),
            (59, 24),
            (63, 32),
            (62, 40),
            (56, 46),
            (45, 50),
            (32, 54),
            (19, 58),
        ];
        let corners = [
            (0, 0),
            (15, -3),
            (30, -9),
            (48, -14),
            (61, -14),
            (69, -8),
            (74, 2),
            (77, 15),
            (81, 30),
            (84, 45),
            (85, 61),
        ];
        for (gesture, a, b) in TWO_LEG {
            let reversal = a.0 == -b.0 && a.1 == -b.1;
            for polarity in if reversal { &[-1., 1.][..] } else { &[1.][..] } {
                let side = if reversal {
                    (-a.1 * polarity, a.0 * polarity)
                } else {
                    b
                };
                for density in [1, 4, 12] {
                    let mut stroke = Stroke::new(-180, 90);
                    let points = if reversal { &returns } else { &corners };
                    for pair in points.windows(2) {
                        for step in 1..=density {
                            let f = f64::from(step) / f64::from(density);
                            let along = f64::from(pair[0].0) + f * f64::from(pair[1].0 - pair[0].0);
                            let cross = f64::from(pair[0].1) + f * f64::from(pair[1].1 - pair[0].1);
                            let jitter = if density > 1 { step % 3 - 1 } else { 0 };
                            stroke.sample(
                                -180 + (along * a.0 + cross * side.0).round() as i32 + jitter,
                                90 + (along * a.1 + cross * side.1).round() as i32,
                            );
                        }
                    }
                    assert_eq!(
                        stroke.gesture(),
                        Some(gesture),
                        "{gesture:?}, density {density}"
                    );
                }
            }
        }
    }
    #[test]
    fn broad_loops_and_shallow_diagonal_bends_are_not_special_gestures() {
        for path in [
            vec![(0, -60), (60, -60), (60, 0), (0, 0)],
            vec![(14, -20), (35, -44), (55, -63), (79, -81)],
            vec![(0, -70), (0, -42), (0, -78), (0, -44)],
        ] {
            let mut stroke = Stroke::new(0, 0);
            for (x, y) in path {
                stroke.sample(x, y);
            }
            assert!(stroke.gesture().is_none_or(|g| g.legs().is_none()));
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
