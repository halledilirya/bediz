#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn spanning(a: Point, b: Point) -> Self {
        Self {
            left: a.x.min(b.x),
            top: a.y.min(b.y),
            right: a.x.max(b.x).saturating_add(1),
            bottom: a.y.max(b.y).saturating_add(1),
        }
    }

    pub fn width(&self) -> u32 {
        u32::try_from(self.right.saturating_sub(self.left)).unwrap_or(0)
    }

    pub fn height(&self) -> u32 {
        u32::try_from(self.bottom.saturating_sub(self.top)).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    pub fn clamp(&self, point: Point) -> Point {
        if self.is_empty() {
            return Point::new(self.left, self.top);
        }
        Point::new(
            point.x.clamp(self.left, self.right.saturating_sub(1)),
            point.y.clamp(self.top, self.bottom.saturating_sub(1)),
        )
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let rect = Rect::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        );
        (!rect.is_empty()).then_some(rect)
    }

    pub fn union(&self, other: &Rect) -> Rect {
        Rect::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }

    pub fn translated(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.left.saturating_add(dx),
            self.top.saturating_add(dy),
            self.right.saturating_add(dx),
            self.bottom.saturating_add(dy),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanning_covers_both_corner_pixels_in_any_drag_direction() {
        let expected = Rect::new(10, 20, 31, 41);
        assert_eq!(
            Rect::spanning(Point::new(10, 20), Point::new(30, 40)),
            expected
        );
        assert_eq!(
            Rect::spanning(Point::new(30, 40), Point::new(10, 20)),
            expected
        );
        assert_eq!(
            Rect::spanning(Point::new(30, 20), Point::new(10, 40)),
            expected
        );
        assert_eq!(expected.width(), 21);
        assert_eq!(expected.height(), 21);
    }

    #[test]
    fn spanning_handles_negative_virtual_screen_coordinates() {
        let rect = Rect::spanning(Point::new(-1920, -100), Point::new(-1, 50));
        assert_eq!(rect, Rect::new(-1920, -100, 0, 51));
        assert_eq!(rect.width(), 1920);
    }

    #[test]
    fn spanning_saturates_at_the_edge_of_the_coordinate_space() {
        let rect = Rect::spanning(
            Point::new(i32::MAX, i32::MAX),
            Point::new(i32::MAX, i32::MAX),
        );
        assert_eq!(rect.right, i32::MAX);
        assert!(rect.is_empty());
    }

    #[test]
    fn inverted_rects_have_no_size() {
        let rect = Rect::new(10, 10, 5, 20);
        assert_eq!(rect.width(), 0);
        assert!(rect.is_empty());
        assert!(!rect.contains(Point::new(7, 15)));
    }

    #[test]
    fn contains_excludes_the_right_and_bottom_edges() {
        let rect = Rect::new(0, 0, 100, 50);
        assert!(rect.contains(Point::new(0, 0)));
        assert!(rect.contains(Point::new(99, 49)));
        assert!(!rect.contains(Point::new(100, 10)));
        assert!(!rect.contains(Point::new(10, 50)));
        assert!(!rect.contains(Point::new(-1, 10)));
    }

    #[test]
    fn clamp_keeps_points_on_the_last_pixel() {
        let rect = Rect::new(-1920, 0, 0, 1080);
        assert_eq!(rect.clamp(Point::new(500, 2000)), Point::new(-1, 1079));
        assert_eq!(rect.clamp(Point::new(-5000, -5)), Point::new(-1920, 0));
        assert_eq!(rect.clamp(Point::new(-10, 10)), Point::new(-10, 10));
    }

    #[test]
    fn clamp_on_an_empty_rect_does_not_panic() {
        let rect = Rect::new(5, 5, 5, 5);
        assert_eq!(rect.clamp(Point::new(100, -100)), Point::new(5, 5));
    }

    #[test]
    fn intersection_of_overlapping_and_disjoint_rects() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, -20, 150, 60);
        assert_eq!(a.intersection(&b), Some(Rect::new(50, 0, 100, 60)));
        assert_eq!(a.intersection(&Rect::new(100, 0, 200, 100)), None);
    }

    #[test]
    fn union_spans_monitors_of_different_sizes() {
        let left = Rect::new(-1920, 200, 0, 1280);
        let right = Rect::new(0, 0, 3840, 2160);
        assert_eq!(left.union(&right), Rect::new(-1920, 0, 3840, 2160));
    }

    #[test]
    fn translated_moves_into_monitor_local_coordinates() {
        let monitor = Rect::new(-1920, -200, 0, 880);
        let selection = Rect::new(-1000, 0, -500, 300);
        assert_eq!(
            selection.translated(-monitor.left, -monitor.top),
            Rect::new(920, 200, 1420, 500)
        );
    }
}
