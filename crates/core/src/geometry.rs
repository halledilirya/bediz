//! Points, rectangles and lengths in virtual screen coordinates, a box set beside an area, and the
//! distance a press moves before it is a drag.

use serde::{Deserialize, Serialize};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

pub fn length(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragDistance {
    pub width: i32,
    pub height: i32,
}

impl DragDistance {
    pub fn passed(self, from: Point, to: Point) -> bool {
        from.x.abs_diff(to.x) > self.width.max(1).unsigned_abs()
            || from.y.abs_diff(to.y) > self.height.max(1).unsigned_abs()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Above,
    Below,
}

pub fn beside(
    area: Rect,
    clear_of: Option<Rect>,
    within: Rect,
    (width, height): (i32, i32),
    gap: i32,
    first: Side,
) -> Point {
    let middle = |rect: &Rect| rect.top.saturating_add(rect.bottom) / 2;
    let area_width = i32::try_from(area.width()).unwrap_or(i32::MAX);
    let x = area
        .left
        .saturating_add(area_width.saturating_sub(width) / 2)
        .min(within.right.saturating_sub(width))
        .max(within.left);
    let fits = |y: i32| {
        let bottom = y.saturating_add(height);
        let placed = Rect::new(x, y, x.saturating_add(width), bottom);
        y >= within.top
            && bottom <= within.bottom
            && clear_of.is_none_or(|other| placed.intersection(&other).is_none())
    };
    let above = area.top.saturating_sub(gap).saturating_sub(height);
    let below = area.bottom.saturating_add(gap);
    let inside_top = area.top.saturating_add(gap);
    let inside_bottom = area.bottom.saturating_sub(gap).saturating_sub(height);
    let (sides, inside) = match first {
        Side::Above => ([above, below], inside_top),
        Side::Below => ([below, above], inside_bottom),
    };
    let y = sides
        .into_iter()
        .find(|&y| fits(y))
        .unwrap_or(match clear_of {
            Some(other) if middle(&other) < middle(&area) => inside_bottom,
            Some(_) => inside_top,
            None => inside,
        });
    Point::new(
        x,
        y.min(within.bottom.saturating_sub(height)).max(within.top),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1040);
    const LABEL: (i32, i32) = (200, 40);

    fn above(area: Rect, clear_of: Option<Rect>) -> Point {
        beside(area, clear_of, WORK, LABEL, 8, Side::Above)
    }

    #[test]
    fn a_press_is_a_drag_once_it_moves_past_the_distance_on_either_axis() {
        let distance = DragDistance {
            width: 4,
            height: 2,
        };
        let from = Point::new(10, 10);
        assert!(!distance.passed(from, Point::new(14, 12)));
        assert!(!distance.passed(from, Point::new(6, 8)));
        assert!(distance.passed(from, Point::new(15, 10)));
        assert!(distance.passed(from, Point::new(10, 7)));
        let unset = DragDistance {
            width: 0,
            height: -3,
        };
        assert!(!unset.passed(from, Point::new(11, 9)));
        assert!(unset.passed(from, Point::new(12, 10)));
        assert!(distance.passed(Point::new(i32::MIN, 0), Point::new(i32::MAX, 0)));
    }

    #[test]
    fn a_box_beside_an_area_goes_above_it_clear_of_the_bar_below() {
        let area = Rect::new(100, 100, 900, 600);
        let bar = Rect::new(400, 610, 600, 650);
        assert_eq!(above(area, Some(bar)), Point::new(400, 52));
        assert_eq!(
            above(Rect::new(100, 20, 900, 600), None),
            Point::new(400, 608)
        );
        assert_eq!(
            above(Rect::new(1800, 300, 1900, 400), None),
            Point::new(1720, 252)
        );
    }

    #[test]
    fn a_box_with_no_room_beside_an_area_goes_inside_its_edge_away_from_the_bar() {
        let low = Rect::new(100, 900, 900, 1030);
        let bar_above = Rect::new(400, 850, 600, 892);
        assert_eq!(above(low, Some(bar_above)), Point::new(400, 982));
        let bar_inside = Rect::new(860, 990, 1060, 1032);
        assert_eq!(above(WORK, Some(bar_inside)), Point::new(860, 8));
        assert_eq!(above(WORK, None), Point::new(860, 8));
        assert_eq!(
            beside(WORK, None, WORK, LABEL, 8, Side::Below),
            Point::new(860, 992)
        );
    }

    #[test]
    fn a_box_beside_an_area_at_the_edge_of_the_numbers_stays_in_its_bounds() {
        let far = Rect::new(i32::MIN, i32::MIN, i32::MIN + 10, i32::MIN + 10);
        assert_eq!(above(far, None), Point::new(0, 0));
        let wide = Rect::new(i32::MIN, 0, i32::MAX, 1040);
        assert_eq!(above(wide, None).y, 8);
    }

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

    #[test]
    fn a_length_past_the_coordinate_space_stops_at_its_edge() {
        assert_eq!(length(1920), 1920);
        assert_eq!(length(u32::MAX), i32::MAX);
    }
}
