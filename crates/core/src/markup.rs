//! Marking up a selection before it is copied or saved: the marks drawn, selecting, moving and
//! reshaping them, and undoing or redoing each change.

pub mod composition;
pub mod effect;
mod gesture;
pub mod ring;
pub mod shape;
mod text;
mod tools;
pub mod typing;

pub use tools::{Colour, Swatch, Tool};

use crate::geometry::Point;

use composition::Composition;
use gesture::Gesture;
use typing::Cursor;

const HISTORY: usize = 100;
pub const THINNEST: f32 = 1.0;
pub const THICKEST: f32 = 40.0;
pub const DEFAULT_WIDTH: f32 = 3.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Pen(Vec<Point>),
    Highlighter(Vec<Point>),
    Line(Point, Point),
    Arrow(Point, Point),
    Rectangle(Point, Point),
    Ellipse(Point, Point),
    Text {
        at: Point,
        text: String,
        extent: Option<Extent>,
    },
    Step {
        at: Point,
        number: u32,
    },
    Redact(Point, Point),
    Blur(Point, Point),
    Pixelate(Point, Point),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub across: f32,
    pub down: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub id: u64,
    pub shape: Shape,
    pub colour: Colour,
    pub width: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Markup {
    marks: Vec<Mark>,
    undone: Vec<Vec<Mark>>,
    redone: Vec<Vec<Mark>>,
    before: Option<Vec<Mark>>,
    next_id: u64,
    gesture: Option<Gesture>,
    selected: Option<u64>,
    editing: Option<u64>,
    cursor: Cursor,
    composition: Option<Composition>,
    revision: u64,
}

impl Markup {
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_busy(&self) -> bool {
        self.gesture.is_some()
    }

    pub fn can_undo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redone.is_empty()
    }

    pub fn selected(&self) -> Option<&Mark> {
        self.find(self.selected?)
    }

    pub fn editing(&self) -> Option<&Mark> {
        self.find(self.editing?)
    }

    pub fn settled(&self) -> usize {
        let held = self.gesture.as_ref().and_then(Gesture::mark);
        [self.selected, self.editing, held]
            .into_iter()
            .flatten()
            .filter_map(|id| self.marks.iter().position(|mark| mark.id == id))
            .min()
            .unwrap_or(self.marks.len())
    }

    pub fn deselect(&mut self) -> bool {
        if self.selected.take().is_none() {
            return false;
        }
        self.changed();
        true
    }

    pub fn back_out(&mut self) -> bool {
        if self.gesture.is_some() {
            self.abandon();
            true
        } else if self.editing.is_some() {
            self.end_editing();
            true
        } else {
            self.deselect()
        }
    }

    pub fn mark_at(&self, at: Point, reach: i32) -> Option<u64> {
        self.marks
            .iter()
            .rev()
            .find(|mark| shape::touches(mark, at, reach))
            .map(|mark| mark.id)
    }

    pub fn delete_selected(&mut self) -> bool {
        let Some(id) = self.selected.take() else {
            return false;
        };
        let earlier = self.marks.clone();
        self.marks.retain(|mark| mark.id != id);
        self.record(earlier);
        self.changed();
        true
    }

    pub fn nudge_selected(&mut self, dx: i32, dy: i32) -> bool {
        let Some(id) = self.selected else {
            return false;
        };
        let earlier = self.marks.clone();
        if let Some(mark) = self.find_mut(id) {
            mark.shape = shape::moved(&mark.shape, dx, dy);
        }
        self.record(earlier);
        self.changed();
        true
    }

    pub fn recolour(&mut self, colour: Colour) -> bool {
        self.restyle(|mark| mark.colour = colour)
    }

    pub fn rewidth(&mut self, width: f32) -> bool {
        let width = width.clamp(THINNEST, THICKEST);
        self.restyle(|mark| mark.width = width)
    }

    pub fn undo(&mut self) -> bool {
        self.settle();
        let Some(previous) = self.undone.pop() else {
            return false;
        };
        self.redone
            .push(std::mem::replace(&mut self.marks, previous));
        self.forget_missing();
        self.changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        self.settle();
        let Some(next) = self.redone.pop() else {
            return false;
        };
        self.undone.push(std::mem::replace(&mut self.marks, next));
        self.forget_missing();
        self.changed();
        true
    }

    fn settle(&mut self) {
        self.end_editing();
        if self.gesture.is_some() {
            self.abandon();
        }
    }

    fn restyle(&mut self, change: impl Fn(&mut Mark)) -> bool {
        let Some(id) = self.editing.or(self.selected) else {
            return false;
        };
        let earlier = self.editing.is_none().then(|| self.marks.clone());
        if let Some(mark) = self.find_mut(id) {
            change(mark);
        }
        if let Some(earlier) = earlier {
            self.record(earlier);
        }
        self.changed();
        true
    }

    fn begin(&mut self) {
        self.before = Some(self.marks.clone());
    }

    fn commit(&mut self) -> bool {
        match self.before.take() {
            Some(before) => self.record(before),
            None => false,
        }
    }

    fn record(&mut self, earlier: Vec<Mark>) -> bool {
        if earlier == self.marks {
            return false;
        }
        self.undone.push(earlier);
        if self.undone.len() > HISTORY {
            self.undone.remove(0);
        }
        self.redone.clear();
        true
    }

    fn forget_missing(&mut self) {
        let marks = &self.marks;
        let exists = |id: Option<u64>| id.filter(|id| marks.iter().any(|mark| mark.id == *id));
        let (selected, editing) = (exists(self.selected), exists(self.editing));
        self.selected = selected;
        self.editing = editing;
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn find(&self, id: u64) -> Option<&Mark> {
        self.marks.iter().find(|mark| mark.id == id)
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Mark> {
        self.marks.iter_mut().find(|mark| mark.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{DragDistance, Rect};

    const RED: Colour = Swatch::Red.colour();
    const SCREEN: Rect = Rect::new(0, 0, 1000, 1000);

    fn drawn(markup: &mut Markup, tool: Tool, from: Point, to: Point) {
        markup.draw(tool, from, RED, 3.0);
        markup.finish(to, false);
    }

    #[test]
    fn marks_are_settled_up_to_the_first_one_drawn_held_selected_or_typed() {
        let mut markup = Markup::default();
        assert_eq!(markup.settled(), 0);
        drawn(&mut markup, Tool::Line, Point::new(0, 0), Point::new(40, 0));
        assert_eq!(markup.settled(), 0);
        let distance = DragDistance {
            width: 4,
            height: 4,
        };
        markup.press(Tool::Pen, Point::new(0, 20), RED, 3.0, distance);
        assert_eq!(markup.settled(), 0);
        markup.drag(Point::new(10, 20), false);
        assert_eq!(markup.settled(), 1);
        assert!(markup.finish(Point::new(40, 30), false));
        assert_eq!(markup.settled(), 1);
        assert!(markup.deselect());
        assert_eq!(markup.settled(), 2);
        assert!(markup.grab(Point::new(20, 0), 2, SCREEN));
        assert_eq!(markup.settled(), 0);
        assert!(markup.finish(Point::new(20, 50), false));
        assert!(markup.deselect());
        assert_eq!(markup.settled(), 2);
        markup.draw(Tool::Text, Point::new(100, 100), RED, 3.0);
        markup.type_text("a");
        assert_eq!(markup.settled(), 2);
        markup.end_editing();
        assert!(markup.deselect());
        assert_eq!(markup.settled(), 3);
    }

    #[test]
    fn undo_and_redo_walk_through_each_change() {
        let mut markup = Markup::default();
        drawn(&mut markup, Tool::Line, Point::new(0, 0), Point::new(10, 0));
        drawn(&mut markup, Tool::Line, Point::new(0, 5), Point::new(10, 5));
        assert_eq!(markup.marks().len(), 2);
        assert!(markup.undo());
        assert_eq!(markup.marks().len(), 1);
        assert!(markup.undo());
        assert!(markup.is_empty());
        assert!(!markup.undo());
        assert!(markup.redo());
        assert!(markup.redo());
        assert_eq!(markup.marks().len(), 2);
        assert!(!markup.redo());
        assert!(markup.undo());
        drawn(&mut markup, Tool::Pen, Point::new(1, 1), Point::new(2, 2));
        assert!(!markup.can_redo());
    }

    #[test]
    fn colour_and_width_change_the_selected_mark_within_bounds() {
        let mut markup = Markup::default();
        drawn(&mut markup, Tool::Line, Point::new(0, 0), Point::new(40, 0));
        assert!(markup.deselect());
        assert!(!markup.recolour(Swatch::Blue.colour()));
        assert!(markup.grab(Point::new(20, 0), 2, SCREEN));
        markup.finish(Point::new(20, 0), false);
        assert!(markup.recolour(Swatch::Blue.colour()));
        assert!(markup.rewidth(500.0));
        let mark = markup.selected().cloned();
        assert_eq!(
            mark.as_ref().map(|mark| mark.colour),
            Some(Swatch::Blue.colour())
        );
        assert_eq!(mark.map(|mark| mark.width), Some(THICKEST));
        assert!(markup.undo());
        assert_eq!(markup.marks().first().map(|mark| mark.width), Some(3.0));
    }

    #[test]
    fn backing_out_drops_the_drag_then_the_typing_then_the_selection() {
        let mut markup = Markup::default();
        drawn(&mut markup, Tool::Line, Point::new(0, 0), Point::new(40, 0));
        assert!(markup.grab_selected(Point::new(20, 0), 2));
        markup.drag(Point::new(20, 30), false);
        assert!(markup.back_out());
        assert!(!markup.is_busy());
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Line(Point::new(0, 0), Point::new(40, 0)))
        );
        assert!(markup.back_out());
        assert!(markup.selected().is_none());
        assert!(!markup.back_out());
        markup.draw(Tool::Text, Point::new(9, 9), RED, 3.0);
        markup.type_text("hi");
        assert!(markup.back_out());
        assert!(markup.editing().is_none());
        assert!(markup.selected().is_some());
        assert!(markup.back_out());
        assert!(!markup.back_out());
        assert_eq!(markup.marks().len(), 2);
        markup.draw(Tool::Pen, Point::new(1, 1), RED, 3.0);
        markup.drag(Point::new(5, 5), false);
        assert!(markup.back_out());
        assert_eq!(markup.marks().len(), 2);
        assert!(!markup.back_out());
    }

    #[test]
    fn nudging_and_deleting_act_on_the_selected_mark_only() {
        let mut markup = Markup::default();
        drawn(
            &mut markup,
            Tool::Arrow,
            Point::new(0, 0),
            Point::new(20, 0),
        );
        assert!(markup.deselect());
        assert!(!markup.nudge_selected(1, 0));
        assert!(!markup.delete_selected());
        assert!(markup.grab(Point::new(10, 0), 2, SCREEN));
        markup.finish(Point::new(10, 0), false);
        assert!(markup.nudge_selected(0, 10));
        assert_eq!(
            markup.selected().map(|mark| &mark.shape),
            Some(&Shape::Arrow(Point::new(0, 10), Point::new(20, 10)))
        );
        assert!(markup.delete_selected());
        assert!(markup.is_empty());
        assert!(markup.undo());
        assert_eq!(markup.marks().len(), 1);
    }

    #[test]
    fn the_history_is_bounded() {
        let mut markup = Markup::default();
        for x in 0..(HISTORY as i32 + 20) {
            drawn(&mut markup, Tool::Line, Point::new(x, 0), Point::new(x, 9));
        }
        let mut undone = 0;
        while markup.undo() {
            undone += 1;
        }
        assert_eq!(undone, HISTORY);
        assert_eq!(markup.marks().len(), 20);
    }

    #[test]
    fn changes_that_change_nothing_keep_what_can_be_redone() {
        let mut markup = Markup::default();
        drawn(&mut markup, Tool::Line, Point::new(0, 0), Point::new(40, 0));
        markup.draw(Tool::Text, Point::new(100, 100), RED, 3.0);
        markup.type_text("note");
        markup.end_editing();
        drawn(
            &mut markup,
            Tool::Rectangle,
            Point::new(0, 50),
            Point::new(40, 90),
        );
        assert!(markup.undo());
        assert!(markup.grab(Point::new(20, 0), 2, SCREEN));
        assert!(!markup.finish(Point::new(20, 0), false));
        assert!(markup.recolour(RED));
        assert!(markup.rewidth(3.0));
        let Some(text) = markup.text_at(Point::new(103, 105), 2) else {
            panic!("the text is under the pointer");
        };
        markup.edit_text(text);
        markup.end_editing();
        assert!(markup.grab(Point::new(20, 0), 2, SCREEN));
        markup.drag(Point::new(20, 30), false);
        markup.abandon();
        drawn(&mut markup, Tool::Arrow, Point::new(5, 5), Point::new(5, 5));
        assert!(markup.can_redo());
        assert!(markup.redo());
        assert_eq!(markup.marks().len(), 3);
    }

    #[test]
    fn a_change_that_changes_nothing_takes_no_step_from_a_full_history() {
        let mut markup = Markup::default();
        for x in 0..HISTORY as i32 {
            drawn(
                &mut markup,
                Tool::Line,
                Point::new(x * 10, 0),
                Point::new(x * 10, 9),
            );
        }
        for _ in 0..5 {
            assert!(markup.grab(Point::new(0, 5), 1, SCREEN));
            assert!(!markup.finish(Point::new(0, 5), false));
        }
        let mut undone = 0;
        while markup.undo() {
            undone += 1;
        }
        assert_eq!(undone, HISTORY);
        assert!(markup.is_empty());
    }
}
