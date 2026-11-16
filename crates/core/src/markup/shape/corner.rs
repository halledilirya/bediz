//! The corners that reshape a mark: where they lie, which way each one faces, and dragging one,
//! which moves an end of a line or a box and scales a text or a step from the opposite corner.

use crate::geometry::Point;

use super::{ends_mut, step_half, text_box, text_pad, text_rect, text_size};
use crate::markup::{Mark, Shape, THICKEST, THINNEST};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corner {
    pub x_from_start: bool,
    pub y_from_start: bool,
}

pub fn corners(mark: &Mark) -> Vec<(Corner, Point)> {
    let corner = |x_from_start, y_from_start| Corner {
        x_from_start,
        y_from_start,
    };
    let four = |from: Point, to: Point| {
        vec![
            (corner(true, true), from),
            (corner(false, true), Point::new(to.x, from.y)),
            (corner(false, false), to),
            (corner(true, false), Point::new(from.x, to.y)),
        ]
    };
    match &mark.shape {
        Shape::Line(from, to) | Shape::Arrow(from, to) => {
            vec![(corner(true, true), *from), (corner(false, false), *to)]
        }
        Shape::Rectangle(from, to)
        | Shape::Ellipse(from, to)
        | Shape::Redact(from, to)
        | Shape::Blur(from, to)
        | Shape::Pixelate(from, to) => four(*from, *to),
        Shape::Text { at, text, extent } => {
            let around = text_rect(*at, text, *extent, mark.width);
            four(
                Point::new(around.left, around.top),
                Point::new(around.right - 1, around.bottom - 1),
            )
        }
        Shape::Step { at, .. } => {
            let half = step_half(mark.width);
            four(
                Point::new(at.x.saturating_sub(half), at.y.saturating_sub(half)),
                Point::new(at.x.saturating_add(half), at.y.saturating_add(half)),
            )
        }
        Shape::Pen(_) | Shape::Highlighter(_) => Vec::new(),
    }
}

pub fn corner_at(mark: &Mark, at: Point, reach: i32) -> Option<Corner> {
    corners(mark)
        .into_iter()
        .find_map(|(corner, point)| within(point, at, reach).then_some(corner))
}

pub fn corner_point(mark: &Mark, corner: Corner) -> Option<Point> {
    corners(mark)
        .into_iter()
        .find_map(|(other, point)| (other == corner).then_some(point))
}

pub fn corner_facing(mark: &Mark, at: Point, reach: i32) -> Option<(i32, i32)> {
    let corners = corners(mark);
    let (_, corner) = corners
        .iter()
        .find(|(_, point)| within(*point, at, reach))?;
    let count = i64::try_from(corners.len()).ok()?;
    let (sum_x, sum_y) = corners.iter().fold((0i64, 0i64), |(x, y), (_, point)| {
        (x + i64::from(point.x), y + i64::from(point.y))
    });
    Some((
        (i64::from(corner.x) * count - sum_x).signum() as i32,
        (i64::from(corner.y) * count - sum_y).signum() as i32,
    ))
}

pub fn reshape(mark: &mut Mark, corner: Corner, to: Point) {
    if corner_point(mark, corner) == Some(to) {
        return;
    }
    let opposite = corner_point(
        mark,
        Corner {
            x_from_start: !corner.x_from_start,
            y_from_start: !corner.y_from_start,
        },
    );
    let width = mark.width;
    match &mut mark.shape {
        Shape::Text { at, text, extent } => {
            let Some(anchor) = opposite else {
                return;
            };
            let around = text_rect(*at, text, *extent, width);
            let span = |from: i32, to: i32| (to - from - 1).max(1) as f32;
            let (span_x, span_y) = (
                span(around.left, around.right),
                span(around.top, around.bottom),
            );
            let (dx, dy) = (
                to.x.abs_diff(anchor.x) as f32,
                to.y.abs_diff(anchor.y) as f32,
            );
            let scale = (dx * span_x + dy * span_y) / (span_x * span_x + span_y * span_y);
            mark.width = width_for_text_size(text_size(width) * scale);
            let pad = text_pad(mark.width);
            let (wide, tall) = text_box(text, *extent, mark.width);
            let start = |anchor: i32, outer: i32, from_start: bool| {
                let edge = if from_start {
                    anchor.saturating_sub(outer - 1)
                } else {
                    anchor
                };
                edge.saturating_add(pad)
            };
            *at = Point::new(
                start(anchor.x, wide + 2 * pad, corner.x_from_start),
                start(anchor.y, tall + 2 * pad, corner.y_from_start),
            );
        }
        Shape::Step { at, .. } => {
            let Some(anchor) = opposite else {
                return;
            };
            let side = to.x.abs_diff(anchor.x).max(to.y.abs_diff(anchor.y)) as f32;
            mark.width = width_for_step_radius(side / 2.0 - 1.0);
            let half = step_half(mark.width);
            let away = |anchor: i32, from_start: bool| {
                if from_start {
                    anchor.saturating_sub(half)
                } else {
                    anchor.saturating_add(half)
                }
            };
            *at = Point::new(
                away(anchor.x, corner.x_from_start),
                away(anchor.y, corner.y_from_start),
            );
        }
        shape => {
            if let Some((from, end)) = ends_mut(shape) {
                if corner.x_from_start {
                    from.x = to.x;
                } else {
                    end.x = to.x;
                }
                if corner.y_from_start {
                    from.y = to.y;
                } else {
                    end.y = to.y;
                }
            }
        }
    }
}

fn width_for_text_size(size: f32) -> f32 {
    ((size - 6.0) / 4.0).clamp(THINNEST, THICKEST)
}

fn width_for_step_radius(radius: f32) -> f32 {
    ((radius - 6.0) / 3.0).clamp(THINNEST, THICKEST)
}

fn within(point: Point, at: Point, reach: i32) -> bool {
    (point.x - at.x).abs() <= reach && (point.y - at.y).abs() <= reach
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::Colour;
    use crate::markup::shape::bounds;

    fn mark(shape: Shape) -> Mark {
        Mark {
            id: 1,
            shape,
            colour: Colour::new(255, 0, 0),
            width: 3.0,
        }
    }

    #[test]
    fn a_corner_faces_away_from_the_middle_of_its_shape() {
        let rectangle = mark(Shape::Rectangle(Point::new(10, 10), Point::new(50, 30)));
        assert_eq!(
            corner_facing(&rectangle, Point::new(11, 9), 2),
            Some((-1, -1))
        );
        assert_eq!(
            corner_facing(&rectangle, Point::new(50, 10), 2),
            Some((1, -1))
        );
        assert_eq!(
            corner_facing(&rectangle, Point::new(50, 30), 2),
            Some((1, 1))
        );
        assert_eq!(corner_facing(&rectangle, Point::new(30, 20), 2), None);
        let line = mark(Shape::Line(Point::new(0, 5), Point::new(40, 5)));
        assert_eq!(corner_facing(&line, Point::new(0, 5), 2), Some((-1, 0)));
        assert_eq!(corner_facing(&line, Point::new(40, 6), 2), Some((1, 0)));
    }

    #[test]
    fn a_box_has_four_corners_and_a_line_two_ends() {
        let rectangle = mark(Shape::Rectangle(Point::new(0, 0), Point::new(10, 20)));
        let points: Vec<Point> = corners(&rectangle).into_iter().map(|(_, at)| at).collect();
        assert_eq!(
            points,
            [
                Point::new(0, 0),
                Point::new(10, 0),
                Point::new(10, 20),
                Point::new(0, 20)
            ]
        );
        let arrow = mark(Shape::Arrow(Point::new(1, 2), Point::new(3, 4)));
        assert_eq!(corners(&arrow).len(), 2);
        assert!(corners(&mark(Shape::Pen(vec![Point::new(0, 0)]))).is_empty());
    }

    #[test]
    fn a_text_and_a_step_have_the_corners_of_the_box_around_them() {
        let text = mark(Shape::Text {
            at: Point::new(100, 100),
            text: "abcd".to_owned(),
            extent: None,
        });
        let around = bounds(&text);
        let points: Vec<Point> = corners(&text).into_iter().map(|(_, at)| at).collect();
        assert_eq!(points.first(), Some(&Point::new(around.left, around.top)));
        assert_eq!(
            points.get(2),
            Some(&Point::new(around.right - 1, around.bottom - 1))
        );
        let step = mark(Shape::Step {
            at: Point::new(50, 50),
            number: 1,
        });
        let half = step_half(3.0);
        assert_eq!(
            corner_point(
                &step,
                Corner {
                    x_from_start: true,
                    y_from_start: true
                }
            ),
            Some(Point::new(50 - half, 50 - half))
        );
    }

    #[test]
    fn dragging_a_corner_moves_only_its_coordinates() {
        let mut rectangle = mark(Shape::Rectangle(Point::new(0, 0), Point::new(10, 20)));
        let Some(corner) = corner_at(&rectangle, Point::new(11, 1), 2) else {
            panic!("the top right corner is near");
        };
        reshape(&mut rectangle, corner, Point::new(30, -5));
        assert_eq!(
            rectangle.shape,
            Shape::Rectangle(Point::new(0, -5), Point::new(30, 20))
        );
        assert_eq!(corner_at(&rectangle, Point::new(15, 10), 2), None);
    }

    #[test]
    fn dragging_a_text_corner_scales_it_and_keeps_the_opposite_corner() {
        let top_left = Corner {
            x_from_start: true,
            y_from_start: true,
        };
        let bottom_right = Corner {
            x_from_start: false,
            y_from_start: false,
        };
        let mut text = mark(Shape::Text {
            at: Point::new(100, 100),
            text: "abcd".to_owned(),
            extent: None,
        });
        let (Some(start), Some(corner)) = (
            corner_point(&text, top_left),
            corner_point(&text, bottom_right),
        ) else {
            panic!("a text has four corners");
        };
        reshape(
            &mut text,
            bottom_right,
            Point::new(
                start.x + 2 * (corner.x - start.x),
                start.y + 2 * (corner.y - start.y),
            ),
        );
        assert!((text.width - 7.5).abs() < 0.01);
        assert_eq!(corner_point(&text, top_left), Some(start));
        let anchor = corner_point(&text, bottom_right);
        reshape(&mut text, top_left, Point::new(140, 130));
        assert!(text.width < 7.5);
        assert_eq!(corner_point(&text, bottom_right), anchor);
        reshape(&mut text, top_left, anchor.unwrap_or_default());
        assert_eq!(text.width, THINNEST);
    }

    #[test]
    fn dragging_a_step_corner_resizes_it_and_keeps_the_opposite_corner() {
        let mut step = mark(Shape::Step {
            at: Point::new(50, 50),
            number: 1,
        });
        let bottom_right = Corner {
            x_from_start: false,
            y_from_start: false,
        };
        reshape(&mut step, bottom_right, Point::new(96, 96));
        assert_eq!(step.width, 8.0);
        assert_eq!(
            step.shape,
            Shape::Step {
                at: Point::new(65, 65),
                number: 1
            }
        );
        let unchanged = step.clone();
        reshape(&mut step, bottom_right, Point::new(96, 96));
        assert_eq!(step, unchanged);
    }
}
