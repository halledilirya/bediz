//! What one overlay frame shows: the selection or the window under the pointer with its size, the
//! magnifier, the marks and words drawn, and a pen's ring and width beside the pointer.

use bediz_core::geometry::{Point, Rect};
use bediz_core::markup::ring;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub selection: Option<Rect>,
    pub handles: bool,
    pub highlight: Option<Rect>,
    pub label: Option<(u32, u32)>,
    pub pointer: Option<Point>,
    pub marks: Option<u64>,
    pub words: Option<u64>,
    pub brush: Option<Brush>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brush {
    pub at: Point,
    pub across: u32,
    pub ring: bool,
    pub width: Option<u32>,
}

impl View {
    pub fn framed(&self) -> Option<(Rect, bool)> {
        self.selection
            .map(|area| (area, self.handles))
            .or(self.highlight.map(|area| (area, false)))
    }
}

impl Brush {
    pub fn reaches(&self, monitor: Rect) -> bool {
        if !self.ring {
            return monitor.contains(self.at);
        }
        monitor
            .intersection(&ring::around(self.at, self.across as f32))
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_has_handles_only_when_its_view_asks_and_a_window_never() {
        let area = Rect::new(10, 20, 110, 220);
        let selected = |handles| View {
            selection: Some(area),
            handles,
            ..View::default()
        };
        assert_eq!(selected(true).framed(), Some((area, true)));
        assert_eq!(selected(false).framed(), Some((area, false)));
        let window = View {
            highlight: Some(area),
            handles: true,
            ..View::default()
        };
        assert_eq!(window.framed(), Some((area, false)));
        assert_eq!(View::default().framed(), None);
    }

    #[test]
    fn a_ring_across_two_monitors_is_drawn_on_both_and_a_small_one_on_its_own() {
        let left = Rect::new(0, 0, 1920, 1080);
        let right = Rect::new(1920, 0, 4480, 1440);
        let brush = |x, across| Brush {
            at: Point::new(x, 500),
            across,
            ring: true,
            width: None,
        };
        assert!(brush(1900, 200).reaches(left));
        assert!(brush(1900, 200).reaches(right));
        assert!(brush(1800, 200).reaches(left));
        assert!(!brush(1800, 200).reaches(right));
        assert!(brush(1919, 0).reaches(left));
        assert!(!brush(1915, 0).reaches(right));
    }

    #[test]
    fn a_width_beside_a_ring_the_cursor_carries_is_drawn_only_where_the_pointer_is() {
        let left = Rect::new(0, 0, 1920, 1080);
        let right = Rect::new(1920, 0, 4480, 1440);
        let width = |x| Brush {
            at: Point::new(x, 500),
            across: 120,
            ring: false,
            width: Some(24),
        };
        assert!(width(1900).reaches(left));
        assert!(!width(1900).reaches(right));
        assert!(width(1920).reaches(right));
        assert!(!width(1920).reaches(left));
    }
}
