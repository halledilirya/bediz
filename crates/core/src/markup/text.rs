//! The text marks of a markup: finding one under the pointer, typing into it with its caret,
//! its selection and an input method's composition, and keeping the size the platform measured.

use crate::geometry::Point;

use super::composition::{self, Composed, Composition};
use super::typing::{self, Cursor, Motion};
use super::{Extent, Mark, Markup, Shape, shape};

impl Markup {
    pub fn text_at(&self, at: Point, reach: i32) -> Option<u64> {
        self.marks
            .iter()
            .rev()
            .find(|mark| {
                matches!(mark.shape, Shape::Text { .. }) && shape::touches(mark, at, reach)
            })
            .map(|mark| mark.id)
    }

    pub fn edit_text(&mut self, id: u64) {
        self.end_editing();
        let Some(Shape::Text { text, .. }) = self.find(id).map(|mark| &mark.shape) else {
            return;
        };
        self.cursor = Cursor::at(text.len());
        self.selected = None;
        self.begin();
        self.editing = Some(id);
        self.changed();
    }

    pub fn cursor(&self) -> Option<Cursor> {
        self.editing.map(|_| self.cursor)
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.editing_text()?
            .get(self.cursor.range())
            .filter(|part| !part.is_empty())
    }

    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    pub fn composed(&self) -> Option<Composed> {
        composition::composed(self.editing()?, self.cursor, self.composition.as_ref()?)
    }

    pub fn compose(&mut self, composition: Option<Composition>) {
        let composition = composition
            .filter(|composition| self.editing.is_some() && !composition.text.is_empty());
        if composition != self.composition {
            self.composition = composition;
            self.changed();
        }
    }

    pub fn finish_composing(&mut self, result: &str) {
        self.compose(None);
        self.type_text(result);
    }

    // Anything else done to the text takes the composition as it stands, as an edit box does.
    pub fn settle_composition(&mut self) {
        if let Some(composition) = self.composition.take() {
            self.type_text(&composition.text);
        }
    }

    pub fn type_text(&mut self, typed: &str) {
        self.settle_composition();
        let typed: String = typed
            .replace("\r\n", "\n")
            .chars()
            .map(|character| if character == '\r' { '\n' } else { character })
            .filter(|character| *character == '\n' || !character.is_control())
            .collect();
        if !typed.is_empty() {
            self.retype(|text, cursor| typing::insert(text, cursor, &typed));
        }
    }

    pub fn erase(&mut self, forward: bool, word: bool) {
        self.settle_composition();
        self.retype(|text, cursor| typing::erase(text, cursor, forward, word));
    }

    pub fn move_caret(&mut self, motion: Motion, extend: bool) {
        self.settle_composition();
        let Some(text) = self.editing_text() else {
            return;
        };
        let cursor = typing::moved(text, self.cursor, motion, extend);
        self.cursor = cursor;
        self.changed();
    }

    pub fn select_all_text(&mut self) {
        self.settle_composition();
        let Some(text) = self.editing_text() else {
            return;
        };
        let cursor = typing::everything(text);
        self.cursor = cursor;
        self.changed();
    }

    pub fn place_caret(&mut self, index: usize) {
        self.settle_composition();
        let Some(text) = self.editing_text() else {
            return;
        };
        let cursor = typing::placed(text, index);
        self.cursor = cursor;
        self.changed();
    }

    pub fn measure_texts(&mut self, measure: impl Fn(&str) -> Option<Extent>) {
        let mut measured = false;
        for mark in &mut self.marks {
            if let Shape::Text {
                text,
                extent: extent @ None,
                ..
            } = &mut mark.shape
            {
                *extent = measure(text);
                measured |= extent.is_some();
            }
        }
        if measured {
            self.changed();
        }
    }

    pub fn end_editing(&mut self) {
        self.settle_composition();
        let Some(id) = self.editing.take() else {
            return;
        };
        self.keep_or_drop(id);
        self.commit();
        self.changed();
    }

    fn editing_text(&self) -> Option<&str> {
        match &self.find(self.editing?)?.shape {
            Shape::Text { text, .. } => Some(text),
            _ => None,
        }
    }

    fn retype(&mut self, change: impl FnOnce(&mut String, Cursor) -> Cursor) {
        let Some(id) = self.editing else {
            return;
        };
        let mut cursor = self.cursor;
        if let Some(Mark {
            shape: Shape::Text { text, extent, .. },
            ..
        }) = self.find_mut(id)
        {
            cursor = change(text, cursor);
            *extent = None;
        }
        self.cursor = cursor;
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::{Colour, Swatch, Tool};

    const RED: Colour = Swatch::Red.colour();

    #[test]
    fn typing_builds_a_text_and_an_empty_one_is_dropped() {
        let mut markup = Markup::default();
        markup.draw(Tool::Text, Point::new(5, 5), RED, 3.0);
        markup.type_text("Hi\r");
        markup.type_text("ü\u{7}😀");
        markup.erase(false, false);
        markup.type_text("!");
        assert_eq!(
            markup.editing().map(|mark| &mark.shape),
            Some(&Shape::Text {
                at: Point::new(5, 5),
                text: "Hi\nü!".to_owned(),
                extent: None,
            })
        );
        markup.end_editing();
        assert!(markup.editing().is_none());
        assert_eq!(markup.marks().len(), 1);
        markup.draw(Tool::Text, Point::new(50, 50), RED, 3.0);
        markup.end_editing();
        assert_eq!(markup.marks().len(), 1);
        assert!(markup.undo());
        assert!(markup.is_empty());
    }

    #[test]
    fn typing_follows_the_caret_and_the_selection() {
        let mut markup = Markup::default();
        markup.draw(Tool::Text, Point::new(5, 5), RED, 3.0);
        markup.type_text("one two\r\nthree");
        assert_eq!(markup.cursor(), Some(Cursor::at(13)));
        markup.move_caret(Motion::WordLeft, true);
        assert_eq!(markup.selected_text(), Some("three"));
        markup.type_text("3");
        markup.move_caret(Motion::Start, false);
        markup.erase(true, true);
        markup.select_all_text();
        assert_eq!(markup.selected_text(), Some("two\n3"));
        markup.place_caret(3);
        assert_eq!(markup.selected_text(), None);
        markup.erase(false, true);
        assert_eq!(
            markup.editing().map(|mark| &mark.shape),
            Some(&Shape::Text {
                at: Point::new(5, 5),
                text: "\n3".to_owned(),
                extent: None,
            })
        );
        markup.end_editing();
        assert_eq!(markup.cursor(), None);
        assert_eq!(markup.selected_text(), None);
    }

    #[test]
    fn a_measured_text_keeps_its_size_until_its_words_change() {
        let mut markup = Markup::default();
        markup.draw(Tool::Text, Point::new(5, 5), RED, 3.0);
        markup.type_text("wide");
        let size = Extent {
            across: 2.5,
            down: 1.25,
        };
        markup.measure_texts(|text| (!text.is_empty()).then_some(size));
        let extent = |markup: &Markup| match markup.editing().map(|mark| &mark.shape) {
            Some(Shape::Text { extent, .. }) => *extent,
            _ => None,
        };
        assert_eq!(extent(&markup), Some(size));
        let revision = markup.revision();
        markup.measure_texts(|_| None);
        assert_eq!(markup.revision(), revision);
        assert_eq!(extent(&markup), Some(size));
        markup.type_text("r");
        assert_eq!(extent(&markup), None);
    }

    #[test]
    fn a_text_is_edited_again_and_an_unchanged_edit_leaves_no_step() {
        let mut markup = Markup::default();
        markup.draw(Tool::Text, Point::new(5, 5), RED, 3.0);
        markup.type_text("note");
        markup.end_editing();
        let Some(id) = markup.text_at(Point::new(8, 10), 2) else {
            panic!("the text is under the pointer");
        };
        markup.edit_text(id);
        markup.end_editing();
        assert!(markup.undo());
        assert!(markup.is_empty());
    }

    fn composing(text: &str) -> Option<Composition> {
        let units: Vec<u16> = text.encode_utf16().collect();
        Composition::from_input_method(&units, &[], &[], units.len())
    }

    fn typed(markup: &Markup) -> Option<&str> {
        match markup.editing().map(|mark| &mark.shape) {
            Some(Shape::Text { text, .. }) => Some(text),
            _ => None,
        }
    }

    fn written(text: &str) -> Shape {
        Shape::Text {
            at: Point::new(5, 5),
            text: text.to_owned(),
            extent: None,
        }
    }

    #[test]
    fn a_composition_is_shown_and_only_its_result_is_typed_in_one_go() {
        let mut markup = Markup::default();
        markup.draw(Tool::Text, Point::new(5, 5), RED, 3.0);
        markup.type_text("ab");
        markup.compose(composing("にほ"));
        assert_eq!(typed(&markup), Some("ab"));
        let shown = markup.composed().map(|composed| composed.mark.shape);
        assert_eq!(shown, Some(written("abにほ")));
        markup.finish_composing("日本");
        assert_eq!(typed(&markup), Some("ab日本"));
        assert_eq!(markup.composition(), None);
        assert_eq!(markup.cursor(), Some(Cursor::at(8)));
        markup.end_editing();
        assert!(markup.undo());
        assert!(markup.is_empty());
    }

    #[test]
    fn a_composition_left_open_is_typed_by_whatever_the_text_does_next() {
        let mut markup = Markup::default();
        markup.compose(composing("ね"));
        assert_eq!(markup.composition(), None);
        markup.draw(Tool::Text, Point::new(5, 5), RED, 3.0);
        markup.compose(composing("ね"));
        markup.type_text("!");
        assert_eq!(typed(&markup), Some("ね!"));
        markup.compose(composing("こ"));
        markup.move_caret(Motion::Start, false);
        assert_eq!(markup.cursor(), Some(Cursor::at(0)));
        markup.compose(composing("ど"));
        markup.end_editing();
        assert_eq!(markup.composition(), None);
        assert_eq!(
            markup.marks().first().map(|mark| &mark.shape),
            Some(&written("どね!こ"))
        );
    }
}
