//! The ring a pen or highlighter shows at the pointer: the edge of what a stroke would cover, in a
//! dark line with a light one outside it, around a small cross with an open centre; the monitors
//! the ring reaches; and the stroke's width each tool shows while it changes.

use super::{Tool, shape};
use crate::geometry::{Point, Rect};

const SHORTEST_ARM: i64 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Clear,
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    side: usize,
    inks: Vec<Ink>,
}

impl Pixels {
    pub fn side(&self) -> usize {
        self.side
    }

    pub fn centre(&self) -> usize {
        self.side / 2
    }

    pub fn rows(&self) -> impl Iterator<Item = &[Ink]> {
        self.inks.chunks(self.side.max(1))
    }
}

pub fn footprint(tool: Tool, width: f32) -> Option<f32> {
    match tool {
        Tool::Pen => Some(width),
        Tool::Highlighter => Some(shape::highlighter_width(width)),
        _ => None,
    }
}

pub fn stroke(tool: Tool, width: f32) -> Option<f32> {
    match tool {
        Tool::Line | Tool::Arrow | Tool::Rectangle | Tool::Ellipse => Some(width),
        _ => footprint(tool, width),
    }
}

pub fn fits(footprint: f32, largest: usize) -> bool {
    let radius = f64::from(footprint) / 2.0;
    radius >= 0.0 && outer_reach(radius) <= reach_limit(largest) as f64
}

pub fn around(at: Point, footprint: f32) -> Rect {
    let reach = outer_reach(f64::from(footprint) / 2.0).max(0.0) as i32;
    Rect::new(
        at.x.saturating_sub(reach),
        at.y.saturating_sub(reach),
        at.x.saturating_add(reach).saturating_add(1),
        at.y.saturating_add(reach).saturating_add(1),
    )
}

pub fn spills(at: Point, footprint: f32, monitors: &[Rect]) -> bool {
    let around = around(at, footprint);
    monitors
        .iter()
        .any(|monitor| !monitor.contains(at) && monitor.intersection(&around).is_some())
}

pub fn pixels(footprint: Option<f32>, arm: u32, largest: usize) -> Pixels {
    let limit = reach_limit(largest);
    let arm = i64::from(arm).min(limit - 1).max(SHORTEST_ARM);
    let gap = (arm / 4).max(2);
    let radius = footprint
        .filter(|across| fits(*across, largest))
        .map(|across| f64::from(across) / 2.0);
    let reach = radius
        .map_or(0, |radius| outer_reach(radius) as i64)
        .max(arm + 1);
    let side = 2 * reach + 1;
    let inks = (0..side * side)
        .map(|at| ink_at(at % side - reach, at / side - reach, radius, arm, gap))
        .collect();
    Pixels {
        side: usize::try_from(side).unwrap_or(0),
        inks,
    }
}

fn reach_limit(largest: usize) -> i64 {
    i64::try_from(largest.saturating_sub(1) / 2).unwrap_or(i64::MAX)
}

fn outer_reach(radius: f64) -> f64 {
    (radius + 2.0).floor()
}

fn ink_at(x: i64, y: i64, radius: Option<f64>, arm: i64, gap: i64) -> Ink {
    let from_centre = (x * x + y * y) as f64;
    let within = |by: f64| radius.is_some_and(|radius| from_centre <= (radius + by).powi(2));
    let on_cross = |x: i64, y: i64| {
        let along = |offset: i64| (gap..=arm).contains(&offset.abs());
        (y == 0 && along(x)) || (x == 0 && along(y))
    };
    if on_cross(x, y) || (within(1.0) && !within(0.0)) {
        return Ink::Dark;
    }
    let open_centre = x.abs() < gap && y.abs() < gap;
    let beside_cross = (-1..=1).any(|dy| (-1..=1).any(|dx| on_cross(x + dx, y + dy)));
    if (beside_cross && !open_centre) || (within(2.0) && !within(1.0)) {
        Ink::Light
    } else {
        Ink::Clear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(pixels: &Pixels, x: i64, y: i64) -> Ink {
        let centre = pixels.centre() as i64;
        let row = pixels.rows().nth((centre + y) as usize);
        row.and_then(|row| row.get((centre + x) as usize).copied())
            .unwrap_or(Ink::Clear)
    }

    #[test]
    fn a_pen_rings_its_own_width_and_a_highlighter_five_times_it() {
        assert_eq!(footprint(Tool::Pen, 3.0), Some(3.0));
        assert_eq!(footprint(Tool::Highlighter, 3.0), Some(15.0));
        assert_eq!(footprint(Tool::Rectangle, 3.0), None);
        assert_eq!(footprint(Tool::Text, 3.0), None);
    }

    #[test]
    fn the_width_shown_is_the_stroke_each_tool_draws() {
        assert_eq!(stroke(Tool::Pen, 4.0), Some(4.0));
        assert_eq!(stroke(Tool::Highlighter, 4.0), Some(20.0));
        for tool in [Tool::Line, Tool::Arrow, Tool::Rectangle, Tool::Ellipse] {
            assert_eq!(stroke(tool, 4.0), Some(4.0));
        }
        for tool in [
            Tool::Text,
            Tool::Step,
            Tool::Redact,
            Tool::Blur,
            Tool::Pixelate,
        ] {
            assert_eq!(stroke(tool, 4.0), None);
        }
    }

    #[test]
    fn a_ring_fits_while_it_and_its_light_line_stay_within_the_cursor() {
        assert!(fits(120.0, 127));
        assert!(fits(123.0, 127));
        assert!(!fits(124.0, 127));
        assert!(!fits(200.0, 127));
        assert!(!fits(f32::NAN, 127));
        assert!(!fits(-1.0, 127));
        assert_eq!(pixels(Some(123.0), 10, 127).side(), 127);
        assert_eq!(pixels(Some(124.0), 10, 127), pixels(None, 10, 127));
    }

    #[test]
    fn the_dark_line_lies_just_outside_what_a_stroke_covers_and_the_light_one_outside_it() {
        let pixels = pixels(Some(20.0), 10, 127);
        assert_eq!(ink(&pixels, 6, 8), Ink::Clear);
        assert_eq!(ink(&pixels, 7, 8), Ink::Dark);
        assert_eq!(ink(&pixels, 8, 8), Ink::Light);
        assert_eq!(ink(&pixels, 9, 8), Ink::Clear);
    }

    #[test]
    fn what_a_stroke_covers_stays_clear_so_the_pixels_under_it_show() {
        for across in [3.0, 20.0] {
            let pixels = pixels(Some(across), 10, 127);
            for (x, y) in [(0, 0), (1, 0), (0, -1), (1, 1)] {
                assert_eq!(ink(&pixels, x, y), Ink::Clear, "{across} px at {x}, {y}");
            }
        }
        let thinnest = pixels(Some(1.0), 10, 127);
        assert_eq!(ink(&thinnest, 0, 0), Ink::Clear);
        assert_eq!(ink(&thinnest, 1, 0), Ink::Dark);
        assert_eq!(ink(&thinnest, -1, 1), Ink::Dark);
    }

    #[test]
    fn the_cross_is_dark_from_its_gap_to_its_arm_with_a_light_edge() {
        let pixels = pixels(None, 10, 127);
        assert_eq!(pixels.side(), 23);
        assert_eq!(ink(&pixels, 2, 0), Ink::Dark);
        assert_eq!(ink(&pixels, 0, -10), Ink::Dark);
        assert_eq!(ink(&pixels, 5, 1), Ink::Light);
        assert_eq!(ink(&pixels, 11, 0), Ink::Light);
        assert_eq!(ink(&pixels, 12, 0), Ink::Clear);
        assert_eq!(ink(&pixels, 5, 5), Ink::Clear);
    }

    #[test]
    fn the_pixels_are_square_odd_and_centred_on_the_pointer() {
        for across in [1.0, 2.0, 15.0, 40.0, 100.0] {
            let pixels = pixels(Some(across), 10, 127);
            assert_eq!(pixels.side() % 2, 1);
            assert_eq!(pixels.rows().count(), pixels.side());
            assert!(pixels.rows().all(|row| row.len() == pixels.side()));
            assert_eq!(pixels.centre() * 2 + 1, pixels.side());
        }
    }

    #[test]
    fn a_ring_too_large_for_a_cursor_leaves_only_the_cross() {
        let large = pixels(Some(200.0), 10, 127);
        assert_eq!(large, pixels(None, 10, 127));
        let fitting = pixels(Some(120.0), 10, 127);
        assert_eq!(fitting.side(), 125);
        assert_eq!(ink(&fitting, 61, 0), Ink::Dark);
    }

    #[test]
    fn a_ring_covers_the_square_out_to_its_light_line_as_its_cursor_does() {
        let at = Point::new(100, 50);
        assert_eq!(around(at, 120.0), Rect::new(38, -12, 163, 113));
        assert_eq!(around(at, 121.0), around(at, 120.0));
        assert_eq!(
            around(at, 120.0).width() as usize,
            pixels(Some(120.0), 10, 127).side()
        );
        assert_eq!(around(at, f32::NAN), Rect::new(100, 50, 101, 51));
        assert_eq!(around(at, -3.0), Rect::new(100, 50, 101, 51));
    }

    #[test]
    fn a_ring_spills_only_where_it_reaches_another_monitor() {
        let side_by_side = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 4480, 1440)];
        let spills_at = |x, y| spills(Point::new(x, y), 120.0, &side_by_side);
        assert!(!spills_at(1857, 500));
        assert!(spills_at(1858, 500));
        assert!(spills_at(1981, 500));
        assert!(!spills_at(1982, 500));
        assert!(spills_at(1950, 1100));
        assert!(!spills_at(1950, 1400));
        assert!(!spills_at(10, 10));
        assert!(!spills_at(4470, 1430));
        assert!(!spills(Point::new(1917, 500), 1.0, &side_by_side));
        assert!(spills(Point::new(1918, 500), 1.0, &side_by_side));
        let stacked = [Rect::new(0, 0, 1920, 1080), Rect::new(0, 1080, 1920, 2160)];
        assert!(spills(Point::new(500, 1050), 120.0, &stacked));
        assert!(!spills(Point::new(500, 1000), 120.0, &stacked));
        assert!(!spills(Point::new(500, 1050), 120.0, &stacked[..1]));
    }

    #[test]
    fn the_cross_shrinks_to_fit_and_never_vanishes() {
        assert!(pixels(None, 500, 127).side() <= 127);
        assert_eq!(pixels(None, 0, 127).side(), 2 * 5 + 1);
        assert_eq!(pixels(Some(f32::NAN), 10, 127), pixels(None, 10, 127));
        assert_eq!(pixels(Some(f32::INFINITY), 10, 127), pixels(None, 10, 127));
        assert_eq!(pixels(Some(-3.0), 10, 127), pixels(None, 10, 127));
    }
}
