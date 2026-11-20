//! The words and QR codes read from the selection, shown only while the selection and its marks
//! stay as they were when it was read: a run of words chosen with the mouse, or all of them, and
//! their text.

use std::ops::RangeInclusive;

use bediz_core::geometry::{Point, Rect};
use bediz_core::markup::{Mark, Markup};
use bediz_core::ocr::{Layout, Passage, Turn};
use bediz_core::qr::Code;
use bediz_core::selection::Grip;

use super::Session;
use super::pointer::beyond_drag_threshold;
use crate::windows::scene::Scene;

pub struct Content {
    selection: Rect,
    revision: u64,
    marks: Vec<Mark>,
}

pub struct Found {
    pub passage: Passage,
    pub turn: Option<Turn>,
    pub codes: Vec<Code>,
}

pub struct Words {
    content: Content,
    found: Found,
    rects: Vec<Rect>,
    code_rects: Vec<Rect>,
    chosen: Option<Chosen>,
    press: Option<Press>,
    stamp: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Chosen {
    anchor: usize,
    focus: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Press {
    at: Point,
    anchor: Option<usize>,
    dragged: bool,
}

impl Session {
    pub fn content(&self) -> Option<Content> {
        if self.marks_busy() {
            return None;
        }
        Some(Content::of(self.settled()?, self.marking.markup()))
    }

    pub fn shows(&self, content: &Content) -> bool {
        content.shown_by(self.selected(), self.marking.markup())
    }

    pub fn shows_words(&self) -> bool {
        self.words.is_some()
    }

    pub fn show_words(&mut self, content: Content, found: Found) {
        let marking_up = self.marking.tool().is_some() || self.marks_busy();
        if marking_up || !self.shows(&content) {
            return;
        }
        self.marking.markup_mut().deselect();
        let rects = found
            .passage
            .words()
            .map(|word| word.bounds.covering())
            .collect();
        let code_rects = found.codes.iter().map(Code::covering).collect();
        self.words = Some(Words {
            content,
            found,
            rects,
            code_rects,
            chosen: None,
            press: None,
            stamp: 0,
        });
        self.restamp();
    }

    pub fn leave_words(&mut self) -> bool {
        self.words.take().is_some()
    }

    pub fn words_text(&self, layout: Layout) -> Option<String> {
        let words = self.words.as_ref()?;
        let passage = &words.found.passage;
        let text = match words.chosen {
            Some(chosen) => passage.chosen_text(chosen.range(), layout),
            None => passage.text(layout),
        };
        (!text.is_empty()).then_some(text)
    }

    pub fn codes_to_copy(&self) -> &[Code] {
        self.words.as_ref().map_or(&[], Words::codes_to_copy)
    }

    pub fn chosen_words(&self) -> Option<(usize, usize)> {
        let words = self.words.as_ref().filter(|words| words.press.is_none());
        chosen(words)
    }

    pub(super) fn press_words(&mut self, scene: &Scene, point: Point) -> bool {
        let Some(selection) = self.selection.filter(|selection| !selection.is_dragging()) else {
            return false;
        };
        if self.grip(&selection, point, scene.reach_at(point)) != Some(Grip::Inside) {
            return false;
        }
        let Some(words) = self.words.as_mut() else {
            return false;
        };
        let (x, y) = words.level(point);
        words.press = Some(Press {
            at: point,
            anchor: words.found.passage.nearest(x, y),
            dragged: false,
        });
        true
    }

    pub(super) fn choosing_words(&self) -> bool {
        self.words
            .as_ref()
            .is_some_and(|words| words.press.is_some())
    }

    pub(super) fn drag_words(&mut self, point: Point) -> bool {
        let Some(words) = self.words.as_mut() else {
            return false;
        };
        let Some(press) = words.press.as_mut() else {
            return false;
        };
        if !press.dragged && !beyond_drag_threshold(press.at, point) {
            return false;
        }
        press.dragged = true;
        let anchor = press.anchor;
        let chosen = words.run_to(anchor, point);
        self.choose_words(chosen)
    }

    // The button can come up elsewhere after Alt+Tab, so a click is taken where it was pressed.
    pub(super) fn release_words(&mut self, scene: &Scene) -> bool {
        let Some(words) = self.words.as_mut() else {
            return false;
        };
        let Some(press) = words.press.take() else {
            return false;
        };
        if press.dragged {
            return false;
        }
        let (x, y) = words.level(press.at);
        let reach = scene.reach_at(press.at) as f32;
        let chosen = words.found.passage.under(x, y, reach).map(|at| Chosen {
            anchor: at,
            focus: at,
        });
        self.choose_words(chosen)
    }

    pub(super) fn choose_all_words(&mut self) -> bool {
        let last = self
            .words
            .as_ref()
            .and_then(|words| words.found.passage.len().checked_sub(1));
        let chosen = last.map(|last| Chosen {
            anchor: 0,
            focus: last,
        });
        self.choose_words(chosen)
    }

    fn choose_words(&mut self, chosen: Option<Chosen>) -> bool {
        let Some(words) = self.words.as_mut() else {
            return false;
        };
        if words.chosen == chosen {
            return false;
        }
        words.chosen = chosen;
        self.restamp();
        true
    }

    fn restamp(&mut self) {
        self.words_shown = self.words_shown.wrapping_add(1);
        if let Some(words) = self.words.as_mut() {
            words.stamp = self.words_shown;
        }
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

impl Words {
    fn level(&self, point: Point) -> (f32, f32) {
        let (x, y) = (point.x as f32, point.y as f32);
        self.found.turn.map_or((x, y), |turn| turn.level(x, y))
    }

    fn run_to(&self, anchor: Option<usize>, point: Point) -> Option<Chosen> {
        let (x, y) = self.level(point);
        let focus = self.found.passage.nearest(x, y)?;
        anchor.map(|anchor| Chosen { anchor, focus })
    }

    fn codes_to_copy(&self) -> &[Code] {
        match self.chosen {
            Some(_) => &[],
            None => &self.found.codes,
        }
    }
}

impl Chosen {
    fn range(self) -> RangeInclusive<usize> {
        self.anchor.min(self.focus)..=self.anchor.max(self.focus)
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
    words.map_or(&[], |words| words.rects.as_slice())
}

pub(super) fn code_rects(words: Option<&Words>) -> &[Rect] {
    words.map_or(&[], |words| words.code_rects.as_slice())
}

pub(super) fn turn(words: Option<&Words>) -> Option<Turn> {
    words.and_then(|words| words.found.turn)
}

pub(super) fn chosen(words: Option<&Words>) -> Option<(usize, usize)> {
    let range = words?.chosen?.range();
    Some((*range.start(), *range.end()))
}

pub(super) fn stamp(words: Option<&Words>) -> Option<u64> {
    words.map(|words| words.stamp)
}

#[cfg(test)]
mod tests {
    use bediz_core::markup::{Swatch, Tool};
    use bediz_core::ocr::{Bounds, Script, Word};

    use super::*;

    const SELECTION: Rect = Rect::new(100, 100, 900, 600);

    fn found(turn: Option<Turn>) -> Found {
        let word = |text: &str, left: f32| Word {
            text: text.to_owned(),
            bounds: Bounds {
                left,
                top: 110.0,
                width: 40.0,
                height: 20.0,
            },
        };
        Found {
            passage: Passage::new(
                vec![vec![word("Koyu", 110.0), word("tema", 160.0)]],
                Script::Spaced,
            ),
            turn,
            codes: vec![Code {
                text: "https://example.com".to_owned(),
                corners: [
                    (300.0, 200.0),
                    (380.0, 200.0),
                    (380.0, 280.0),
                    (300.0, 280.0),
                ],
            }],
        }
    }

    fn shown(chosen: Option<Chosen>, turn: Option<Turn>) -> Words {
        Words {
            content: Content::of(SELECTION, &Markup::default()),
            rects: vec![Rect::new(110, 110, 150, 130), Rect::new(160, 110, 200, 130)],
            code_rects: vec![Rect::new(300, 200, 380, 280)],
            found: found(turn),
            chosen,
            press: None,
            stamp: 4,
        }
    }

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
    fn the_highlights_their_turn_and_the_words_chosen_come_from_the_words_shown() {
        assert!(rects(None).is_empty());
        assert!(code_rects(None).is_empty());
        assert_eq!(turn(None), None);
        assert_eq!(chosen(None), None);
        assert_eq!(stamp(None), None);
        let tilt = Turn {
            degrees: 10.0,
            x: 500.0,
            y: 350.0,
        };
        let backwards = Chosen {
            anchor: 1,
            focus: 0,
        };
        let words = shown(Some(backwards), Some(tilt));
        assert_eq!(rects(Some(&words)).len(), 2);
        assert_eq!(code_rects(Some(&words)), [Rect::new(300, 200, 380, 280)]);
        assert_eq!(turn(Some(&words)), Some(tilt));
        assert_eq!(chosen(Some(&words)), Some((0, 1)));
        assert_eq!(chosen(Some(&shown(None, None))), None);
        assert_eq!(stamp(Some(&words)), Some(4));
    }

    #[test]
    fn the_codes_are_copied_unless_words_are_chosen() {
        assert_eq!(shown(None, None).codes_to_copy().len(), 1);
        let chosen = Chosen {
            anchor: 0,
            focus: 0,
        };
        assert!(shown(Some(chosen), None).codes_to_copy().is_empty());
    }

    #[test]
    fn a_drag_runs_from_the_word_it_began_at_to_the_one_nearest_the_pointer() {
        let words = shown(None, None);
        assert_eq!(
            words.run_to(Some(1), Point::new(120, 120)),
            Some(Chosen {
                anchor: 1,
                focus: 0
            })
        );
        assert_eq!(words.run_to(None, Point::new(120, 120)), None);
        assert_eq!(
            Chosen {
                anchor: 3,
                focus: 1
            }
            .range(),
            1..=3
        );
    }

    #[test]
    fn a_point_on_tilted_words_is_turned_back_before_it_is_matched() {
        let tilt = Turn {
            degrees: 90.0,
            x: 150.0,
            y: 120.0,
        };
        let words = shown(None, Some(tilt));
        let (x, y) = words.level(Point::new(150, 150));
        assert!((x - 180.0).abs() < 1e-3 && (y - 120.0).abs() < 1e-3);
        assert_eq!(
            words.run_to(Some(0), Point::new(150, 150)),
            Some(Chosen {
                anchor: 0,
                focus: 1
            })
        );
    }
}
