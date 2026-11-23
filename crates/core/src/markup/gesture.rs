//! Drawing a mark and taking hold of one with the pointer: where a press lands, a press beside the
//! selected mark that only lets go of it unless it is dragged, dragging and letting go, which keeps
//! the mark selected or drops an empty one.

use crate::geometry::{DragDistance, Point, Rect};

use super::shape::{self, Corner};
use super::typing::Cursor;
use super::{Colour, Mark, Markup, Shape, THICKEST, THINNEST, Tool};

#[derive(Clone, Debug)]
pub(super) enum Gesture {
    Wait {
        tool: Tool,
        from: Point,
        colour: Colour,
        width: f32,
        distance: DragDistance,
    },
    Draw {
        id: u64,
        from: Point,
    },
    Move {
        id: u64,
        from: Point,
        original: Shape,
    },
    Reshape {
        corner: Corner,
        from: Point,
        original: Mark,
    },
}

impl Gesture {
    pub(super) fn mark(&self) -> Option<u64> {
        match self {
            Self::Wait { .. } => None,
            Self::Draw { id, .. } | Self::Move { id, .. } => Some(*id),
            Self::Reshape { original, .. } => Some(original.id),
        }
    }
}

impl Markup {
    pub fn press(
        &mut self,
        tool: Tool,
        at: Point,
        colour: Colour,
        width: f32,
        distance: DragDistance,
    ) {
        // A text or a step is made by a click, so a click beside the last one makes the next.
        if self.selected.is_none() || matches!(tool, Tool::Text | Tool::Step) {
            self.draw(tool, at, colour, width);
            return;
        }
        self.gesture = Some(Gesture::Wait {
            tool,
            from: at,
            colour,
            width,
            distance,
        });
    }

    pub fn draw(&mut self, tool: Tool, at: Point, colour: Colour, width: f32) {
        self.end_editing();
        self.selected = None;
        self.begin();
        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        let shape = match tool {
            Tool::Pen => Shape::Pen(vec![at]),
            Tool::Highlighter => Shape::Highlighter(vec![at]),
            Tool::Line => Shape::Line(at, at),
            Tool::Arrow => Shape::Arrow(at, at),
            Tool::Rectangle => Shape::Rectangle(at, at),
            Tool::Ellipse => Shape::Ellipse(at, at),
            Tool::Text => Shape::Text {
                at,
                text: String::new(),
                extent: None,
            },
            Tool::Step => Shape::Step {
                at,
                number: self.next_step(),
            },
            Tool::Redact => Shape::Redact(at, at),
            Tool::Blur => Shape::Blur(at, at),
            Tool::Pixelate => Shape::Pixelate(at, at),
        };
        self.marks.push(Mark {
            id,
            shape,
            colour,
            width: width.clamp(THINNEST, THICKEST),
        });
        if tool == Tool::Text {
            self.editing = Some(id);
            self.cursor = Cursor::default();
        } else {
            self.gesture = Some(Gesture::Draw { id, from: at });
        }
        self.changed();
    }

    pub fn grab(&mut self, at: Point, reach: i32, shown: Rect) -> bool {
        self.end_editing();
        if self.reshape_selected(at, reach) {
            return true;
        }
        let found = shown
            .contains(at)
            .then(|| self.mark_at(at, reach))
            .flatten();
        let Some(id) = found else {
            self.deselect();
            return false;
        };
        self.hold(id, at)
    }

    pub fn grab_selected(&mut self, at: Point, reach: i32) -> bool {
        let held = self
            .editing
            .or(self.selected)
            .and_then(|id| self.find(id))
            .is_some_and(|mark| {
                shape::corner_at(mark, at, reach).is_some()
                    || shape::outline_touches(mark, at, reach)
            });
        if !held {
            return false;
        }
        self.end_editing();
        if self.reshape_selected(at, reach) {
            return true;
        }
        match self.selected {
            Some(id) => self.hold(id, at),
            None => false,
        }
    }

    pub fn drag(&mut self, to: Point, straight: bool) {
        if let Some(&Gesture::Wait {
            tool,
            from,
            colour,
            width,
            distance,
        }) = self.gesture.as_ref()
        {
            if !distance.passed(from, to) {
                return;
            }
            self.gesture = None;
            self.draw(tool, from, colour, width);
        }
        let Some(gesture) = self.gesture.clone() else {
            return;
        };
        match gesture {
            Gesture::Wait { .. } => return,
            Gesture::Draw { id, from } => {
                if let Some(mark) = self.find_mut(id) {
                    extend(&mut mark.shape, from, to, straight);
                }
            }
            Gesture::Move { id, from, original } => {
                if let Some(mark) = self.find_mut(id) {
                    mark.shape = shape::moved(&original, to.x - from.x, to.y - from.y);
                }
            }
            Gesture::Reshape {
                corner,
                from,
                original,
            } => {
                let mut reshaped = original;
                if let Some(start) = shape::corner_point(&reshaped, corner) {
                    let target = Point::new(
                        start.x.saturating_add(to.x - from.x),
                        start.y.saturating_add(to.y - from.y),
                    );
                    shape::reshape(&mut reshaped, corner, target);
                }
                if let Some(mark) = self.find_mut(reshaped.id) {
                    *mark = reshaped;
                }
            }
        }
        self.changed();
    }

    pub fn finish(&mut self, at: Point, straight: bool) -> bool {
        self.drag(at, straight);
        let Some(gesture) = self.gesture.take() else {
            return false;
        };
        match gesture {
            Gesture::Wait { .. } => {
                self.deselect();
                return false;
            }
            Gesture::Draw { id, .. } => self.keep_or_drop(id),
            Gesture::Move { .. } | Gesture::Reshape { .. } => {}
        }
        let changed = self.commit();
        self.changed();
        changed
    }

    pub fn abandon(&mut self) {
        if self.gesture.take().is_some()
            && let Some(before) = self.before.take()
        {
            self.marks = before;
            self.changed();
        }
    }

    pub(super) fn keep_or_drop(&mut self, id: u64) {
        match self.find(id).map(|mark| shape::is_empty(&mark.shape)) {
            Some(true) => self.marks.retain(|mark| mark.id != id),
            Some(false) => self.selected = Some(id),
            None => {}
        }
    }

    fn reshape_selected(&mut self, at: Point, reach: i32) -> bool {
        let Some(selected) = self.selected() else {
            return false;
        };
        let Some(corner) = shape::corner_at(selected, at, reach) else {
            return false;
        };
        let original = selected.clone();
        self.begin();
        self.gesture = Some(Gesture::Reshape {
            corner,
            from: at,
            original,
        });
        true
    }

    fn hold(&mut self, id: u64, at: Point) -> bool {
        let Some(original) = self.find(id).map(|mark| mark.shape.clone()) else {
            return false;
        };
        self.selected = Some(id);
        self.begin();
        self.gesture = Some(Gesture::Move {
            id,
            from: at,
            original,
        });
        self.changed();
        true
    }

    fn next_step(&self) -> u32 {
        self.marks
            .iter()
            .filter_map(|mark| match mark.shape {
                Shape::Step { number, .. } => Some(number),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }
}

fn extend(shape: &mut Shape, from: Point, to: Point, straight: bool) {
    match shape {
        Shape::Pen(points) | Shape::Highlighter(points) => {
            if points.last() != Some(&to) {
                points.push(to);
            }
        }
        Shape::Line(_, end) | Shape::Arrow(_, end) => {
            *end = if straight {
                shape::snapped(from, to)
            } else {
                to
            };
        }
        Shape::Rectangle(_, end)
        | Shape::Ellipse(_, end)
        | Shape::Redact(_, end)
        | Shape::Blur(_, end)
        | Shape::Pixelate(_, end) => {
            *end = if straight {
                shape::squared(from, to)
            } else {
                to
            };
        }
        Shape::Step { at, .. } => *at = to,
        Shape::Text { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::Swatch;

    const RED: Colour = Swatch::Red.colour();
    const SCREEN: Rect = Rect::new(0, 0, 1000, 1000);
    const DISTANCE: DragDistance = DragDistance {
        width: 4,
        height: 4,
    };

    fn drawn(markup: &mut Markup, tool: Tool, from: Point, to: Point) {
        markup.draw(tool, from, RED, 3.0);
        markup.finish(to, false);
    }

    fn pressed(markup: &mut Markup, tool: Tool, from: Point, to: Point) {
        markup.press(tool, from, RED, 3.0, DISTANCE);
        markup.finish(to, false);
    }

    #[test]
    fn a_drawn_arrow_is_kept_and_a_click_without_a_drag_draws_nothing() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Arrow,
            Point::new(10, 10),
            Point::new(60, 40),
        );
        assert_eq!(
            markup.marks().first().map(|mark| &mark.shape),
            Some(&Shape::Arrow(Point::new(10, 10), Point::new(60, 40)))
        );
        drawn(
            &mut markup,
            Tool::Rectangle,
            Point::new(5, 5),
            Point::new(5, 5),
        );
        assert_eq!(markup.marks().len(), 1);
        assert!(markup.undo());
        assert!(markup.is_empty());
        assert!(!markup.can_undo());
    }

    #[test]
    fn a_drawn_mark_stays_selected_until_the_next_one_begins() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Arrow,
            Point::new(10, 10),
            Point::new(60, 40),
        );
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Arrow(Point::new(10, 10), Point::new(60, 40)))
        );
        assert!(markup.rewidth(9.0));
        assert_eq!(markup.selected().map(|mark| mark.width), Some(9.0));
        drawn(
            &mut markup,
            Tool::Rectangle,
            Point::new(90, 90),
            Point::new(90, 90),
        );
        assert!(markup.selected().is_none());
        assert_eq!(markup.marks().len(), 1);
        assert!(!markup.deselect());
    }

    #[test]
    fn a_shift_drag_draws_a_square_and_a_straight_line() {
        let mut markup = Markup::default();
        markup.draw(Tool::Rectangle, Point::new(0, 0), RED, 3.0);
        markup.finish(Point::new(30, 10), true);
        markup.draw(Tool::Line, Point::new(0, 0), RED, 3.0);
        markup.finish(Point::new(50, 4), true);
        let shapes: Vec<&Shape> = markup.marks().iter().map(|mark| &mark.shape).collect();
        assert_eq!(
            shapes,
            [
                &Shape::Rectangle(Point::new(0, 0), Point::new(30, 30)),
                &Shape::Line(Point::new(0, 0), Point::new(50, 0)),
            ]
        );
    }

    #[test]
    fn steps_count_on_from_the_highest_number() {
        let mut markup = Markup::default();
        for x in [10, 60, 110] {
            drawn(
                &mut markup,
                Tool::Step,
                Point::new(x, 10),
                Point::new(x, 10),
            );
        }
        let numbers = |markup: &Markup| -> Vec<u32> {
            markup
                .marks()
                .iter()
                .filter_map(|mark| match mark.shape {
                    Shape::Step { number, .. } => Some(number),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(numbers(&markup), [1, 2, 3]);
        assert!(markup.grab(Point::new(60, 10), 2, SCREEN));
        markup.finish(Point::new(60, 10), false);
        assert!(markup.delete_selected());
        drawn(
            &mut markup,
            Tool::Step,
            Point::new(9, 90),
            Point::new(9, 90),
        );
        assert_eq!(numbers(&markup), [1, 3, 4]);
    }

    #[test]
    fn an_abandoned_drawing_leaves_the_marks_as_they_were() {
        let mut markup = Markup::default();
        drawn(&mut markup, Tool::Line, Point::new(0, 0), Point::new(9, 9));
        markup.draw(Tool::Pen, Point::new(1, 1), RED, 3.0);
        markup.drag(Point::new(5, 5), false);
        markup.abandon();
        assert_eq!(markup.marks().len(), 1);
        assert!(!markup.is_busy());
        assert!(markup.undo());
        assert!(markup.is_empty());
    }

    #[test]
    fn a_click_beside_the_mark_just_drawn_lets_go_of_it_and_draws_nothing() {
        let dragged = [
            Tool::Pen,
            Tool::Highlighter,
            Tool::Line,
            Tool::Arrow,
            Tool::Rectangle,
            Tool::Ellipse,
            Tool::Redact,
            Tool::Blur,
            Tool::Pixelate,
        ];
        for tool in dragged {
            let mut markup = Markup::default();
            pressed(&mut markup, tool, Point::new(10, 10), Point::new(60, 60));
            assert!(markup.selected().is_some(), "{tool:?}");
            markup.press(tool, Point::new(200, 200), RED, 3.0, DISTANCE);
            markup.drag(Point::new(203, 197), false);
            assert!(markup.is_busy());
            assert!(markup.selected().is_some());
            assert!(!markup.finish(Point::new(204, 196), false));
            assert!(!markup.is_busy());
            assert!(markup.selected().is_none(), "{tool:?}");
            assert_eq!(markup.marks().len(), 1, "{tool:?}");
            assert!(markup.undo());
            assert!(markup.marks().is_empty());
            assert!(!markup.can_undo(), "{tool:?}");
        }
    }

    #[test]
    fn a_drag_beside_the_mark_just_drawn_begins_the_next_where_it_was_pressed() {
        let mut markup = Markup::default();
        pressed(
            &mut markup,
            Tool::Pen,
            Point::new(10, 10),
            Point::new(60, 60),
        );
        markup.press(Tool::Pen, Point::new(200, 200), RED, 3.0, DISTANCE);
        markup.drag(Point::new(204, 200), false);
        assert_eq!(markup.marks().len(), 1);
        markup.drag(Point::new(205, 200), false);
        assert!(markup.selected().is_none());
        markup.finish(Point::new(240, 230), false);
        let stroke = [
            Point::new(200, 200),
            Point::new(205, 200),
            Point::new(240, 230),
        ];
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Pen(stroke.to_vec()))
        );
        pressed(
            &mut markup,
            Tool::Rectangle,
            Point::new(300, 300),
            Point::new(340, 320),
        );
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Rectangle(
                Point::new(300, 300),
                Point::new(340, 320)
            ))
        );
        assert_eq!(markup.marks().len(), 3);
    }

    #[test]
    fn a_click_with_nothing_selected_draws_a_dot_and_steps_and_texts_follow_each_click() {
        let mut markup = Markup::default();
        let at = Point::new(5, 5);
        pressed(&mut markup, Tool::Pen, at, at);
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Pen(vec![at]))
        );
        let mut markup = Markup::default();
        for x in [10, 50, 90] {
            let at = Point::new(x, 10);
            pressed(&mut markup, Tool::Step, at, at);
        }
        let numbers: Vec<u32> = markup
            .marks()
            .iter()
            .filter_map(|mark| match mark.shape {
                Shape::Step { number, .. } => Some(number),
                _ => None,
            })
            .collect();
        assert_eq!(numbers, [1, 2, 3]);
        assert!(markup.selected().is_some());
        markup.press(Tool::Text, Point::new(100, 100), RED, 3.0, DISTANCE);
        assert!(markup.editing().is_some());
        assert!(!markup.is_busy());
    }

    #[test]
    fn esc_while_a_press_waits_keeps_the_mark_selected_and_draws_nothing() {
        let mut markup = Markup::default();
        pressed(
            &mut markup,
            Tool::Line,
            Point::new(10, 10),
            Point::new(60, 60),
        );
        markup.press(Tool::Line, Point::new(200, 200), RED, 3.0, DISTANCE);
        assert!(markup.back_out());
        assert!(!markup.is_busy());
        assert!(markup.selected().is_some());
        markup.drag(Point::new(260, 260), false);
        assert!(!markup.finish(Point::new(260, 260), false));
        assert_eq!(markup.marks().len(), 1);
        assert!(markup.selected().is_some());
    }

    #[test]
    fn a_mark_is_grabbed_moved_and_moved_back_by_undo() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Rectangle,
            Point::new(10, 10),
            Point::new(50, 30),
        );
        assert!(markup.grab(Point::new(20, 20), 3, SCREEN));
        markup.finish(Point::new(30, 25), false);
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Rectangle(Point::new(20, 15), Point::new(60, 35)))
        );
        assert!(markup.undo());
        assert_eq!(
            markup.marks().first().map(|mark| &mark.shape),
            Some(&Shape::Rectangle(Point::new(10, 10), Point::new(50, 30)))
        );
    }

    #[test]
    fn grabbing_without_moving_selects_and_leaves_no_step_to_undo() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Ellipse,
            Point::new(0, 0),
            Point::new(40, 40),
        );
        assert!(markup.grab(Point::new(20, 20), 3, SCREEN));
        markup.finish(Point::new(20, 20), false);
        assert!(markup.selected().is_some());
        assert!(markup.undo());
        assert!(markup.is_empty());
        assert!(!markup.grab(Point::new(500, 500), 3, SCREEN));
        assert!(markup.selected().is_none());
    }

    #[test]
    fn the_selected_marks_corner_reshapes_it() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Blur,
            Point::new(10, 10),
            Point::new(50, 50),
        );
        assert!(markup.grab(Point::new(30, 30), 3, SCREEN));
        markup.finish(Point::new(30, 30), false);
        assert!(markup.grab(Point::new(51, 49), 3, SCREEN));
        assert!(markup.finish(Point::new(81, 89), false));
        assert_eq!(
            markup.marks().first().map(|mark| &mark.shape),
            Some(&Shape::Blur(Point::new(10, 10), Point::new(80, 90)))
        );
        assert!(markup.grab(Point::new(79, 91), 3, SCREEN));
        assert!(!markup.finish(Point::new(79, 91), false));
    }

    #[test]
    fn a_step_and_a_text_grow_from_a_corner_and_shrink_back_by_undo() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Step,
            Point::new(50, 50),
            Point::new(50, 50),
        );
        let Some(corner) = markup.selected().map(shape::corners) else {
            panic!("the step is selected");
        };
        let Some((_, bottom_right)) = corner.get(2).copied() else {
            panic!("a step has four corners");
        };
        assert!(markup.grab_selected(bottom_right, 2));
        assert!(markup.finish(Point::new(bottom_right.x + 20, bottom_right.y + 20), false));
        assert!(markup.selected().is_some_and(|mark| mark.width > 3.0));
        markup.draw(Tool::Text, Point::new(200, 200), RED, 3.0);
        markup.type_text("hi");
        markup.end_editing();
        let before = markup.selected().cloned();
        let Some(corner) = markup.selected().map(shape::corners) else {
            panic!("the text is selected");
        };
        let Some((_, bottom_right)) = corner.get(2).copied() else {
            panic!("a text has four corners");
        };
        assert!(markup.grab_selected(bottom_right, 2));
        assert!(markup.finish(Point::new(bottom_right.x + 30, bottom_right.y + 30), false));
        assert!(markup.selected().is_some_and(|mark| mark.width > 3.0));
        assert!(markup.undo());
        assert_eq!(markup.marks().last().cloned(), before);
    }

    #[test]
    fn while_drawing_only_the_selected_mark_is_taken_hold_of() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Rectangle,
            Point::new(10, 10),
            Point::new(110, 60),
        );
        drawn(
            &mut markup,
            Tool::Line,
            Point::new(200, 10),
            Point::new(300, 10),
        );
        assert!(!markup.grab_selected(Point::new(10, 30), 2));
        assert!(markup.grab_selected(Point::new(250, 11), 2));
        assert!(markup.finish(Point::new(250, 51), false));
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Line(Point::new(200, 50), Point::new(300, 50)))
        );
        assert!(markup.grab_selected(Point::new(301, 49), 2));
        assert!(markup.finish(Point::new(321, 79), false));
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Line(Point::new(200, 50), Point::new(320, 80)))
        );
        assert!(markup.undo());
        assert!(markup.undo());
        assert_eq!(
            markup.marks().get(1).map(|mark| &mark.shape),
            Some(&Shape::Line(Point::new(200, 10), Point::new(300, 10)))
        );
    }

    #[test]
    fn inside_a_selected_box_a_new_mark_can_still_begin() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Rectangle,
            Point::new(10, 10),
            Point::new(110, 60),
        );
        assert!(!markup.grab_selected(Point::new(60, 35), 2));
        assert!(markup.grab_selected(Point::new(110, 35), 2));
        assert!(!markup.finish(Point::new(110, 35), false));
        assert!(markup.undo());
        assert!(markup.is_empty());
    }

    #[test]
    fn a_finished_text_is_selected_and_taken_hold_of_while_typing() {
        let mut markup = Markup::default();
        markup.draw(Tool::Text, Point::new(5, 5), RED, 3.0);
        markup.type_text("note");
        assert!(markup.selected().is_none());
        assert!(markup.grab_selected(Point::new(8, 10), 2));
        assert!(markup.editing().is_none());
        assert!(markup.finish(Point::new(28, 40), false));
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Text {
                at: Point::new(25, 35),
                text: "note".to_owned(),
                extent: None,
            })
        );
        markup.draw(Tool::Text, Point::new(200, 200), RED, 3.0);
        markup.end_editing();
        assert!(markup.selected().is_none());
    }

    #[test]
    fn a_press_outside_the_selection_reaches_only_the_selected_marks_corners() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Rectangle,
            Point::new(10, 10),
            Point::new(50, 30),
        );
        assert!(markup.deselect());
        let selection = Rect::new(100, 100, 300, 300);
        assert!(!markup.grab(Point::new(30, 10), 2, selection));
        assert!(!markup.is_busy());
        assert!(markup.selected().is_none());
        assert!(markup.grab(Point::new(30, 10), 2, SCREEN));
        assert!(!markup.finish(Point::new(30, 10), false));
        assert!(markup.grab(Point::new(50, 30), 2, selection));
        assert!(markup.finish(Point::new(60, 40), false));
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Rectangle(Point::new(10, 10), Point::new(60, 40)))
        );
        assert!(!markup.grab(Point::new(30, 10), 2, selection));
        assert!(markup.selected().is_none());
    }
}
