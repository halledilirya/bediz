//! Reading the text in the selection: hands the image as it would be copied to the recognizer's
//! thread one reading at a time, copies what it read and highlights its words, or says why not,
//! then copies the words chosen, or all of them, and again when the lines are laid out anew.

mod turns;

use std::sync::PoisonError;

use bediz_core::delivery::CopyError;
use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use bediz_core::ocr::{Layout, Passage, Script, Word};

use super::Runtime;
use super::copies::Copied;
use crate::event::Event;
use crate::windows::clipboard::Contents;
use crate::windows::ocr::{self as recognizer, ReadError, Reading};
use crate::windows::session::{Content, Found, Session};
use turns::Next;
pub(super) use turns::Turns;

const READ: &str = "Bediz.Text";

pub(super) struct Asked {
    content: Content,
    shown: Rect,
}

impl Runtime {
    pub(super) fn read_text(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session.shows_words() {
            self.reselect(|session, _| {
                session.leave_words();
            });
            return;
        }
        if session.content().is_none() {
            return;
        }
        self.reselect(|session, _| session.marking_mut().put_down());
        let ticket = self.copies.reserve();
        if !self.turns.defer(ticket) {
            self.start_reading(ticket);
        }
    }

    fn start_reading(&mut self, ticket: u64) {
        let Some(content) = self.session.as_ref().and_then(Session::content) else {
            self.emit_notes(vec![
                "the selection was being changed when its text was to be read again".to_owned(),
            ]);
            return;
        };
        let Some((shown, image)) = self.read_selection() else {
            return;
        };
        let image = match image {
            Ok(image) => image,
            Err(error) => {
                let reason = format!("the selection could not be read: {error}");
                self.unread(Notice::TextNotRead(reason), shown);
                return;
            }
        };
        let request = self.turns.request();
        let language = self.text_language.clone();
        match recognizer::read(self.window.0, &self.readings, request, image, language) {
            Ok(()) => self.turns.run(request, Asked { content, shown }, ticket),
            Err(error) => {
                let reason = format!("the thread that reads text could not start: {error}");
                self.unread(Notice::TextNotRead(reason), shown);
            }
        }
    }

    pub(super) fn on_text(&mut self) {
        let read = self
            .readings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(read) = read else {
            return;
        };
        let session = self.session.as_ref();
        let next = self.turns.finish(read.request, |asked| {
            session.is_some_and(|session| session.shows(&asked.content))
        });
        match next {
            None => {}
            Some(Next::Drop) => self.emit_notes(vec![
                "text read from a selection that changed meanwhile was dropped".to_owned(),
            ]),
            Some(Next::ReadAgain { ticket }) => {
                self.emit_notes(vec![
                    "text read from a selection that changed meanwhile was dropped for a new reading"
                        .to_owned(),
                ]);
                self.start_reading(ticket);
            }
            Some(Next::Use { asked, ticket }) => match read.result {
                Ok(reading) => self.copy_read_text(asked, ticket, reading),
                Err(ReadError::NoLanguage) => self.unread(Notice::NoTextLanguage, asked.shown),
                Err(ReadError::LanguageMissing) => {
                    self.unread(Notice::TextLanguageMissing, asked.shown);
                }
                Err(ReadError::Failed(reason)) => {
                    self.unread(Notice::TextNotRead(reason), asked.shown);
                }
                Err(ReadError::NeedsRestart(reason)) => {
                    self.unread(Notice::TextNeedsRestart(reason), asked.shown);
                }
            },
        }
    }

    fn copy_read_text(&mut self, asked: Asked, ticket: u64, reading: Reading) {
        let found = reading.lines.iter().map(Vec::len).sum::<usize>();
        let (width, height) = reading.placement.size();
        self.emit_notes(vec![format!(
            "text read in {} ms and {} passes: {} lines and {found} words in {}, from {width}x{height} at {:.2}x",
            reading.took.as_millis(),
            reading.passes,
            reading.lines.len(),
            reading.language,
            reading.placement.scale(),
        )]);
        let placement = reading.placement;
        let lines = reading
            .lines
            .into_iter()
            .map(|line| {
                line.into_iter()
                    .map(|word| Word {
                        bounds: placement.on_screen(word.bounds, asked.shown),
                        text: word.text,
                    })
                    .collect()
            })
            .collect();
        let passage = Passage::new(lines, Script::of(&reading.language));
        if passage.is_empty() {
            self.unread(Notice::NoTextFound, asked.shown);
            return;
        }
        let contents = text_contents(&passage.text(self.text_layout));
        let found = Found {
            passage,
            turn: placement.turn(asked.shown),
        };
        self.place_on_clipboard(ticket, contents, Copied::ReadText { asked, found }, None);
    }

    pub(super) fn text_copied(&mut self, asked: Asked, found: Found) {
        self.reselect(|session, _| session.show_words(asked.content, found));
        self.tell_text_copied();
    }

    pub(super) fn text_not_copied(&mut self, error: CopyError, asked: &Asked) {
        self.notify_at_once(vec![Notice::CopyFailed(error)], asked.shown);
    }

    pub(super) fn copy_words(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let (Some(text), Some(near)) = (session.words_text(self.text_layout), session.selected())
        else {
            return;
        };
        self.place_words(&text, near);
    }

    pub(super) fn lay_out_text(&mut self, layout: Layout) {
        self.text_layout = layout;
        (self.sink)(Event::TextLayout(layout));
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let (Some(text), Some(near)) = (session.words_text(layout), session.selected()) else {
            return;
        };
        self.place_words(&text, near);
    }

    fn place_words(&mut self, text: &str, near: Rect) {
        let ticket = self.copies.reserve();
        self.place_on_clipboard(ticket, text_contents(text), Copied::Words { near }, None);
    }

    pub(super) fn words_copied(&mut self, near: Rect, result: Result<(), CopyError>) {
        let notice = match result {
            Ok(()) => Notice::TextCopied,
            Err(error) => Notice::CopyFailed(error),
        };
        self.notify_at_once(vec![notice], near);
    }

    fn tell_text_copied(&self) {
        let (Some(scene), Some(texts)) = (self.scene.as_ref(), self.menu_texts.as_ref()) else {
            return;
        };
        if self.session.is_some()
            && let Err(error) = scene.announce(&texts.text_copied, READ)
        {
            self.emit_notes(vec![format!(
                "a screen reader could not be told the text was copied: {error}"
            )]);
        }
    }

    fn unread(&mut self, notice: Notice, near: Rect) {
        self.notify_at_once(vec![notice], near);
    }
}

fn text_contents(text: &str) -> Contents {
    Contents::text(&text.replace('\n', "\r\n"))
}
