//! The geometry of marks: their sizes, where they lie, whether a point touches one or its outline,
//! moving one and the head of an arrow.

mod corner;

pub use corner::{Corner, corner_at, corner_facing, corner_point, corners, reshape};

use crate::geometry::{Point, Rect};

use super::{Extent, Mark, Shape};

const HEAD_SPREAD: f32 = 0.5;
const TEXT_WIDTH_PER_CHARACTER: f32 = 0.6;
const TEXT_LINE_HEIGHT: f32 = 1.3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
}

impl Spot {
    pub fn of(point: Point) -> Self {
        Self {
            x: point.x as f32 + 0.5,
            y: point.y as f32 + 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrowHead {
    pub tip: Spot,
    pub left: Spot,
    pub right: Spot,
    pub shaft_end: Spot,
}

pub fn text_size(width: f32) -> f32 {
    6.0 + width * 4.0
}

pub fn step_radius(width: f32) -> f32 {
    6.0 + width * 3.0
}

pub fn highlighter_width(width: f32) -> f32 {
    width * 5.0
}

pub fn pixel_block(width: f32) -> u32 {
    (4.0 + width * 3.0).round().max(2.0) as u32
}

pub fn blur_radius(width: f32) -> u32 {
    (2.0 + width * 3.0).round().max(1.0) as u32
}

pub fn head_length(width: f32) -> f32 {
    8.0 + width * 4.0
}

pub fn stroke(mark: &Mark) -> f32 {
    match mark.shape {
        Shape::Highlighter(_) => highlighter_width(mark.width),
        _ => mark.width,
    }
}

pub fn arrow_head(from: Point, to: Point, width: f32) -> Option<ArrowHead> {
    let (start, tip) = (Spot::of(from), Spot::of(to));
    let (dx, dy) = (tip.x - start.x, tip.y - start.y);
    let length = dx.hypot(dy);
    if length < 1.0 {
        return None;
    }
    let (along_x, along_y) = (dx / length, dy / length);
    let head = head_length(width).min(length);
    let half = head * HEAD_SPREAD;
    let base = Spot {
        x: tip.x - along_x * head,
        y: tip.y - along_y * head,
    };
    Some(ArrowHead {
        tip,
        left: Spot {
            x: base.x - along_y * half,
            y: base.y + along_x * half,
        },
        right: Spot {
            x: base.x + along_y * half,
            y: base.y - along_x * half,
        },
        shaft_end: Spot {
            x: base.x + along_x * head / 2.0,
            y: base.y + along_y * head / 2.0,
        },
    })
}

pub fn text_extent(text: &str, width: f32) -> (f32, f32) {
    let size = text_size(width);
    let lines = text.split('\n');
    let (count, longest) = lines.fold((0usize, 0usize), |(count, longest), line| {
        (count + 1, longest.max(line.chars().count()))
    });
    (
        longest.max(1) as f32 * size * TEXT_WIDTH_PER_CHARACTER,
        count.max(1) as f32 * size * TEXT_LINE_HEIGHT,
    )
}

pub fn bounds(mark: &Mark) -> Rect {
    let pad = (stroke(mark) / 2.0).ceil() as i32 + 1;
    match &mark.shape {
        Shape::Pen(points) | Shape::Highlighter(points) => {
            let mut points = points.iter();
            let Some(first) = points.next() else {
                return Rect::default();
            };
            let spanned = points.fold(Rect::spanning(*first, *first), |rect, point| {
                rect.union(&Rect::spanning(*point, *point))
            });
            grown(spanned, pad)
        }
        Shape::Arrow(from, to) => {
            let spread = (head_length(mark.width) * HEAD_SPREAD).ceil() as i32;
            grown(Rect::spanning(*from, *to), pad.max(spread))
        }
        Shape::Line(from, to) | Shape::Rectangle(from, to) | Shape::Ellipse(from, to) => {
            grown(Rect::spanning(*from, *to), pad)
        }
        Shape::Redact(from, to) | Shape::Blur(from, to) | Shape::Pixelate(from, to) => {
            Rect::spanning(*from, *to)
        }
        Shape::Text { at, text, extent } => text_rect(*at, text, *extent, mark.width),
        Shape::Step { at, .. } => grown(Rect::spanning(*at, *at), step_half(mark.width)),
    }
}

pub fn touches(mark: &Mark, at: Point, reach: i32) -> bool {
    let near = reach as f32 + stroke(mark) / 2.0;
    let spot = Spot::of(at);
    match &mark.shape {
        Shape::Pen(points) | Shape::Highlighter(points) => match points.as_slice() {
            [only] => distance(spot, Spot::of(*only)) <= near,
            points => points
                .windows(2)
                .filter_map(|pair| match pair {
                    [from, to] => Some(distance_to_segment(spot, *from, *to)),
                    _ => None,
                })
                .any(|distance| distance <= near),
        },
        Shape::Line(from, to) | Shape::Arrow(from, to) => {
            distance_to_segment(spot, *from, *to) <= near
        }
        Shape::Rectangle(from, to)
        | Shape::Ellipse(from, to)
        | Shape::Redact(from, to)
        | Shape::Blur(from, to)
        | Shape::Pixelate(from, to) => grown(Rect::spanning(*from, *to), reach).contains(at),
        Shape::Text { .. } => grown(bounds(mark), reach).contains(at),
        Shape::Step { at: centre, .. } => {
            distance(spot, Spot::of(*centre)) <= step_radius(mark.width) + reach as f32
        }
    }
}

pub fn outline_touches(mark: &Mark, at: Point, reach: i32) -> bool {
    let near = reach as f32 + stroke(mark) / 2.0;
    let spot = Spot::of(at);
    match &mark.shape {
        Shape::Rectangle(from, to) => distance_to_box(spot, *from, *to) <= near,
        Shape::Ellipse(from, to) => distance_to_ellipse(spot, *from, *to) <= near,
        _ => touches(mark, at, reach),
    }
}

pub fn moved(shape: &Shape, dx: i32, dy: i32) -> Shape {
    let by = |point: &Point| Point::new(point.x.saturating_add(dx), point.y.saturating_add(dy));
    match shape {
        Shape::Pen(points) => Shape::Pen(points.iter().map(by).collect()),
        Shape::Highlighter(points) => Shape::Highlighter(points.iter().map(by).collect()),
        Shape::Line(from, to) => Shape::Line(by(from), by(to)),
        Shape::Arrow(from, to) => Shape::Arrow(by(from), by(to)),
        Shape::Rectangle(from, to) => Shape::Rectangle(by(from), by(to)),
        Shape::Ellipse(from, to) => Shape::Ellipse(by(from), by(to)),
        Shape::Redact(from, to) => Shape::Redact(by(from), by(to)),
        Shape::Blur(from, to) => Shape::Blur(by(from), by(to)),
        Shape::Pixelate(from, to) => Shape::Pixelate(by(from), by(to)),
        Shape::Text { at, text, extent } => Shape::Text {
            at: by(at),
            text: text.clone(),
            extent: *extent,
        },
        Shape::Step { at, number } => Shape::Step {
            at: by(at),
            number: *number,
        },
    }
}

pub fn ends_mut(shape: &mut Shape) -> Option<(&mut Point, &mut Point)> {
    match shape {
        Shape::Line(from, to)
        | Shape::Arrow(from, to)
        | Shape::Rectangle(from, to)
        | Shape::Ellipse(from, to)
        | Shape::Redact(from, to)
        | Shape::Blur(from, to)
        | Shape::Pixelate(from, to) => Some((from, to)),
        Shape::Pen(_) | Shape::Highlighter(_) | Shape::Text { .. } | Shape::Step { .. } => None,
    }
}

pub fn is_empty(shape: &Shape) -> bool {
    match shape {
        Shape::Line(from, to) | Shape::Arrow(from, to) => from == to,
        Shape::Rectangle(from, to)
        | Shape::Ellipse(from, to)
        | Shape::Redact(from, to)
        | Shape::Blur(from, to)
        | Shape::Pixelate(from, to) => from.x == to.x || from.y == to.y,
        Shape::Text { text, .. } => text.is_empty(),
        Shape::Pen(points) | Shape::Highlighter(points) => points.is_empty(),
        Shape::Step { .. } => false,
    }
}

pub fn snapped(from: Point, to: Point) -> Point {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let (wide, tall) = (dx.abs(), dy.abs());
    if tall * 5 < wide * 2 {
        Point::new(to.x, from.y)
    } else if wide * 5 < tall * 2 {
        Point::new(from.x, to.y)
    } else {
        squared(from, to)
    }
}

pub fn squared(from: Point, to: Point) -> Point {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let side = dx.abs().max(dy.abs());
    let sign = |delta: i32| if delta < 0 { -1 } else { 1 };
    Point::new(
        from.x.saturating_add(side * sign(dx)),
        from.y.saturating_add(side * sign(dy)),
    )
}

fn grown(rect: Rect, by: i32) -> Rect {
    Rect::new(
        rect.left.saturating_sub(by),
        rect.top.saturating_sub(by),
        rect.right.saturating_add(by),
        rect.bottom.saturating_add(by),
    )
}

fn text_box(text: &str, extent: Option<Extent>, width: f32) -> (i32, i32) {
    let (wide, tall) = match extent {
        Some(extent) => {
            let size = text_size(width);
            (extent.across * size, extent.down * size)
        }
        None => text_extent(text, width),
    };
    ((wide.ceil() as i32).max(2), (tall.ceil() as i32).max(2))
}

// The frame is drawn on the box's outer pixels, so without room it would hide the text's edge.
fn text_pad(width: f32) -> i32 {
    (text_size(width) / 12.0).ceil() as i32 + 1
}

fn text_rect(at: Point, text: &str, extent: Option<Extent>, width: f32) -> Rect {
    let (wide, tall) = text_box(text, extent, width);
    grown(
        Rect::new(
            at.x,
            at.y,
            at.x.saturating_add(wide),
            at.y.saturating_add(tall),
        ),
        text_pad(width),
    )
}

fn step_half(width: f32) -> i32 {
    step_radius(width).ceil() as i32 + 1
}

fn distance(a: Spot, b: Spot) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn distance_to_box(spot: Spot, from: Point, to: Point) -> f32 {
    let (a, b) = (Spot::of(from), Spot::of(to));
    let (left, right) = (a.x.min(b.x), a.x.max(b.x));
    let (top, bottom) = (a.y.min(b.y), a.y.max(b.y));
    let outside_x = (left - spot.x).max(spot.x - right).max(0.0);
    let outside_y = (top - spot.y).max(spot.y - bottom).max(0.0);
    if outside_x > 0.0 || outside_y > 0.0 {
        return outside_x.hypot(outside_y);
    }
    (spot.x - left)
        .min(right - spot.x)
        .min(spot.y - top)
        .min(bottom - spot.y)
}

fn distance_to_ellipse(spot: Spot, from: Point, to: Point) -> f32 {
    let (a, b) = (Spot::of(from), Spot::of(to));
    let (radius_x, radius_y) = ((a.x - b.x).abs() / 2.0, (a.y - b.y).abs() / 2.0);
    if radius_x < 0.5 || radius_y < 0.5 {
        return distance_to_segment(spot, from, to);
    }
    let (dx, dy) = (spot.x - (a.x + b.x) / 2.0, spot.y - (a.y + b.y) / 2.0);
    let scaled = (dx / radius_x).hypot(dy / radius_y);
    if scaled == 0.0 {
        return radius_x.min(radius_y);
    }
    dx.hypot(dy) * (1.0 - 1.0 / scaled).abs()
}

fn distance_to_segment(spot: Spot, from: Point, to: Point) -> f32 {
    let (a, b) = (Spot::of(from), Spot::of(to));
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx * dx + dy * dy;
    if length == 0.0 {
        return distance(spot, a);
    }
    let along = (((spot.x - a.x) * dx + (spot.y - a.y) * dy) / length).clamp(0.0, 1.0);
    distance(
        spot,
        Spot {
            x: a.x + along * dx,
            y: a.y + along * dy,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::Colour;

    fn mark(shape: Shape) -> Mark {
        Mark {
            id: 1,
            shape,
            colour: Colour::new(255, 0, 0),
            width: 3.0,
        }
    }

    #[test]
    fn an_arrow_head_points_along_the_arrow_and_spreads_to_both_sides() {
        let head = arrow_head(Point::new(0, 0), Point::new(100, 0), 3.0);
        let Some(head) = head else {
            panic!("a long arrow has a head");
        };
        assert_eq!(head.tip, Spot { x: 100.5, y: 0.5 });
        let length = head_length(3.0);
        assert!((head.left.x - (100.5 - length)).abs() < 0.01);
        assert!((head.left.y - head.right.y).abs() > length * 0.9);
        assert!(head.shaft_end.x < head.tip.x && head.shaft_end.x > head.left.x);
    }

    #[test]
    fn an_arrow_shorter_than_its_head_gets_a_head_as_long_as_itself() {
        let Some(head) = arrow_head(Point::new(0, 0), Point::new(4, 0), 10.0) else {
            panic!("a short arrow still has a head");
        };
        assert!((head.left.x - 0.5).abs() < 0.01);
        assert_eq!(arrow_head(Point::new(3, 3), Point::new(3, 3), 3.0), None);
    }

    #[test]
    fn a_line_is_touched_near_it_and_not_far_from_it() {
        let line = mark(Shape::Line(Point::new(0, 0), Point::new(100, 100)));
        assert!(touches(&line, Point::new(50, 52), 3));
        assert!(!touches(&line, Point::new(50, 70), 3));
        assert!(!touches(&line, Point::new(130, 130), 3));
    }

    #[test]
    fn a_rectangle_is_touched_inside_and_on_its_edge() {
        let rectangle = mark(Shape::Rectangle(Point::new(10, 10), Point::new(40, 30)));
        assert!(touches(&rectangle, Point::new(20, 20), 0));
        assert!(touches(&rectangle, Point::new(42, 20), 3));
        assert!(!touches(&rectangle, Point::new(50, 20), 3));
    }

    #[test]
    fn only_the_outline_of_a_box_or_an_ellipse_is_touched_by_its_outline() {
        let rectangle = mark(Shape::Rectangle(Point::new(10, 10), Point::new(110, 60)));
        assert!(!outline_touches(&rectangle, Point::new(60, 35), 2));
        assert!(touches(&rectangle, Point::new(60, 35), 2));
        assert!(outline_touches(&rectangle, Point::new(11, 35), 2));
        assert!(outline_touches(&rectangle, Point::new(60, 62), 2));
        assert!(!outline_touches(&rectangle, Point::new(60, 70), 2));
        let ellipse = mark(Shape::Ellipse(Point::new(0, 0), Point::new(100, 50)));
        assert!(!outline_touches(&ellipse, Point::new(50, 25), 2));
        assert!(outline_touches(&ellipse, Point::new(0, 25), 2));
        assert!(outline_touches(&ellipse, Point::new(50, 1), 2));
        assert!(!outline_touches(&ellipse, Point::new(20, 25), 2));
        let line = mark(Shape::Line(Point::new(0, 0), Point::new(100, 100)));
        assert_eq!(
            outline_touches(&line, Point::new(50, 52), 3),
            touches(&line, Point::new(50, 52), 3)
        );
    }

    #[test]
    fn a_pen_stroke_is_touched_along_its_points() {
        let pen = mark(Shape::Pen(vec![
            Point::new(0, 0),
            Point::new(50, 0),
            Point::new(50, 50),
        ]));
        assert!(touches(&pen, Point::new(25, 1), 2));
        assert!(touches(&pen, Point::new(51, 25), 2));
        assert!(!touches(&pen, Point::new(25, 25), 2));
        let dot = mark(Shape::Pen(vec![Point::new(5, 5)]));
        assert!(touches(&dot, Point::new(6, 6), 2));
    }

    #[test]
    fn a_step_is_touched_within_its_circle() {
        let step = mark(Shape::Step {
            at: Point::new(100, 100),
            number: 1,
        });
        let radius = step_radius(3.0) as i32;
        assert!(touches(&step, Point::new(100 + radius - 1, 100), 0));
        assert!(!touches(&step, Point::new(100 + radius + 5, 100), 0));
    }

    #[test]
    fn moving_a_shape_moves_every_point() {
        let pen = Shape::Pen(vec![Point::new(0, 0), Point::new(5, 5)]);
        assert_eq!(
            moved(&pen, 10, -2),
            Shape::Pen(vec![Point::new(10, -2), Point::new(15, 3)])
        );
        let extent = Some(Extent {
            across: 0.5,
            down: 1.2,
        });
        let text = Shape::Text {
            at: Point::new(1, 1),
            text: "a".to_owned(),
            extent,
        };
        assert_eq!(
            moved(&text, 1, 1),
            Shape::Text {
                at: Point::new(2, 2),
                text: "a".to_owned(),
                extent,
            }
        );
    }

    #[test]
    fn a_measured_text_is_framed_by_its_measure_with_room_around_it() {
        let measured = |text: &str| {
            mark(Shape::Text {
                at: Point::new(0, 0),
                text: text.to_owned(),
                extent: Some(Extent {
                    across: 2.0,
                    down: 1.5,
                }),
            })
        };
        let (size, pad) = (text_size(3.0), text_pad(3.0));
        assert!(pad >= 2);
        assert_eq!(
            bounds(&measured("WIDE")),
            Rect::new(
                -pad,
                -pad,
                (2.0 * size).ceil() as i32 + pad,
                (1.5 * size).ceil() as i32 + pad
            )
        );
        let mut wider = measured("WIDE");
        wider.width = 6.0;
        let outer = (2.0 * text_size(6.0)).ceil() as i32 + 2 * text_pad(6.0);
        assert_eq!(bounds(&wider).width(), outer as u32);
    }

    #[test]
    fn a_shift_drag_keeps_lines_straight_or_diagonal_and_boxes_square() {
        let from = Point::new(0, 0);
        assert_eq!(snapped(from, Point::new(100, 10)), Point::new(100, 0));
        assert_eq!(snapped(from, Point::new(-8, 90)), Point::new(0, 90));
        assert_eq!(snapped(from, Point::new(60, 50)), Point::new(60, 60));
        assert_eq!(squared(from, Point::new(-30, 10)), Point::new(-30, 30));
        assert_eq!(squared(from, Point::new(0, 0)), Point::new(0, 0));
        assert_eq!(squared(from, Point::new(0, -7)), Point::new(7, -7));
    }

    #[test]
    fn a_box_without_area_and_a_line_without_length_are_empty() {
        assert!(is_empty(&Shape::Line(Point::new(1, 1), Point::new(1, 1))));
        assert!(is_empty(&Shape::Rectangle(
            Point::new(1, 1),
            Point::new(1, 9)
        )));
        assert!(!is_empty(&Shape::Rectangle(
            Point::new(1, 1),
            Point::new(2, 9)
        )));
        assert!(!is_empty(&Shape::Step {
            at: Point::new(0, 0),
            number: 1
        }));
    }

    #[test]
    fn a_text_grows_with_its_longest_line_and_its_lines() {
        let (one_wide, one_tall) = text_extent("ab", 3.0);
        let (two_wide, two_tall) = text_extent("abcd\nab", 3.0);
        assert!(two_wide > one_wide);
        assert!(two_tall > one_tall);
        let (empty_wide, _) = text_extent("", 3.0);
        assert!(empty_wide > 0.0);
    }

    #[test]
    fn effect_strength_and_sizes_grow_with_the_width() {
        assert!(pixel_block(6.0) > pixel_block(2.0));
        assert!(blur_radius(6.0) > blur_radius(2.0));
        assert!(pixel_block(0.0) >= 2);
        assert!(blur_radius(0.0) >= 1);
        assert!(text_size(4.0) > text_size(2.0));
    }
}
