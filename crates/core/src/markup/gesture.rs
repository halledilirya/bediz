//! Drawing a mark and taking hold of one with the pointer: where a press lands, dragging it and
//! letting it go, which keeps the mark selected or drops an empty one.

use crate::geometry::{Point, Rect};

use super::shape::{self, Corner};
use super::typing::Cursor;
use super::{Colour, Mark, Markup, Shape, THICKEST, THINNEST, Tool};

#[derive(Clone, Debug)]
pub(super) enum Gesture {
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
    pub(super) fn mark(&self) -> u64 {
        match self {
            Self::Draw { id, .. } | Self::Move { id, .. } => *id,
            Self::Reshape { original, .. } => original.id,
        }
    }
}

impl Markup {
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
        let Some(gesture) = self.gesture.clone() else {
            return;
        };
        match gesture {
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
        if let Gesture::Draw { id, .. } = gesture {
            self.keep_or_drop(id);
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
