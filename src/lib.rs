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

impl Direction {
    pub const fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Left => ("向左划", "后退"),
            Self::Right => ("向右划", "前进"),
            Self::Up => ("向上划", "页面顶部"),
            Self::Down => ("向下划", "页面底部"),
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
    fn four_fixed_directions_match_their_preview() {
        for (x, y, label) in [
            (-60, 0, "后退"),
            (60, 0, "前进"),
            (0, -60, "页面顶部"),
            (0, 60, "页面底部"),
        ] {
            let mut stroke = Stroke::new(0, 0);
            stroke.sample(x, y);
            assert_eq!(stroke.direction().map(|d| d.labels().1), Some(label));
        }
    }
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
}
