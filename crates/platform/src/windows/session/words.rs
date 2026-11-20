//! The words read from the selection, shown only while the selection and its marks stay as they
//! were when it was read.

use bediz_core::geometry::Rect;
use bediz_core::markup::{Mark, Markup};
use bediz_core::ocr::Turn;

use super::Session;

pub struct Content {
    selection: Rect,
    revision: u64,
    marks: Vec<Mark>,
}

pub struct Highlights {
    pub rects: Vec<Rect>,
    pub turn: Option<Turn>,
}

pub struct Words {
    content: Content,
    highlights: Highlights,
    id: u64,
}

impl Session {
    pub fn content(&self) -> Option<Content> {
        Some(Content::of(self.settled()?, self.marking.markup()))
    }

    pub fn shows(&self, content: &Content) -> bool {
        content.shown_by(self.selected(), self.marking.markup())
    }

    pub fn show_words(&mut self, content: Content, highlights: Highlights) {
        if !self.shows(&content) {
            return;
        }
        self.words_read = self.words_read.wrapping_add(1);
        self.words = Some(Words {
            content,
            highlights,
            id: self.words_read,
        });
    }

    pub(super) fn forget_stale_words(&mut self) {
        if self
            .words
            .as_ref()
            .is_some_and(|words| !self.shows(&words.content))
        {
            self.words = None;
        }
    }
}

impl Content {
    fn of(selection: Rect, markup: &Markup) -> Self {
        Self {
            selection,
            revision: markup.revision(),
            marks: markup.marks().to_vec(),
        }
    }

    // Choosing or leaving a mark moves the revision without changing a mark.
    fn shown_by(&self, selection: Option<Rect>, markup: &Markup) -> bool {
        selection == Some(self.selection)
            && (markup.revision() == self.revision || markup.marks() == self.marks.as_slice())
    }
}

pub(super) fn rects(words: Option<&Words>) -> &[Rect] {
    words.map_or(&[], |words| words.highlights.rects.as_slice())
}

pub(super) fn turn(words: Option<&Words>) -> Option<Turn> {
    words.and_then(|words| words.highlights.turn)
}

pub(super) fn id(words: Option<&Words>) -> Option<u64> {
    words.map(|words| words.id)
}

#[cfg(test)]
mod tests {
    use bediz_core::geometry::Point;
    use bediz_core::markup::{Swatch, Tool};

    use super::*;

    const SELECTION: Rect = Rect::new(100, 100, 900, 600);

    #[test]
    fn words_stay_while_the_selection_and_its_marks_do() {
        let mut markup = Markup::default();
        markup.draw(Tool::Line, Point::new(200, 200), Swatch::Red.colour(), 3.0);
        markup.finish(Point::new(400, 300), false);
        let content = Content::of(SELECTION, &markup);
        assert!(content.shown_by(Some(SELECTION), &markup));
        assert!(markup.deselect());
        assert!(content.shown_by(Some(SELECTION), &markup));
        assert!(!content.shown_by(Some(Rect::new(100, 100, 900, 601)), &markup));
        assert!(!content.shown_by(None, &markup));
        assert!(markup.undo());
        assert!(!content.shown_by(Some(SELECTION), &markup));
        assert!(markup.redo());
        assert!(content.shown_by(Some(SELECTION), &markup));
    }

    #[test]
    fn the_highlights_and_their_id_come_from_the_words_shown() {
        assert!(rects(None).is_empty());
        assert_eq!(turn(None), None);
        assert_eq!(id(None), None);
        let tilt = Turn {
            degrees: 10.0,
            x: 500.0,
            y: 350.0,
        };
        let words = Words {
            content: Content::of(SELECTION, &Markup::default()),
            highlights: Highlights {
                rects: vec![Rect::new(110, 110, 150, 130)],
                turn: Some(tilt),
            },
            id: 4,
        };
        assert_eq!(rects(Some(&words)), [Rect::new(110, 110, 150, 130)]);
        assert_eq!(turn(Some(&words)), Some(tilt));
        assert_eq!(id(Some(&words)), Some(4));
    }
}
