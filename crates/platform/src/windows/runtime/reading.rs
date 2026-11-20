//! Reading the text and QR codes in the selection: hands the image as it would be copied to the
//! reading thread one reading at a time, copies what a code says or else the text, highlights
//! them or says why not, then copies the words chosen or what was read, and again on a new layout.

mod turns;

use std::sync::PoisonError;

use bediz_core::delivery::CopyError;
use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use bediz_core::ocr::{Layout, Passage, Script, Turn, Word};
use bediz_core::qr::{self, Code};

use super::Runtime;
use super::copies::Copied;
use crate::event::Event;
use crate::windows::clipboard::Contents;
use crate::windows::ocr::{self as recognizer, Looked, Read, ReadError, Reading};
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
            Some(Next::Use { asked, ticket }) => self.use_reading(asked, ticket, read),
        }
    }

    // A code found wins over the text around it, and the text is still shown to choose from.
    fn use_reading(&mut self, asked: Asked, ticket: u64, read: Read) {
        let codes = self.placed_codes(read.codes, asked.shown);
        let (passage, turn) = match read.result {
            Ok(reading) => self.placed_words(reading, asked.shown, &codes),
            Err(error) if codes.is_empty() => {
                self.unread(not_read(error), asked.shown);
                return;
            }
            Err(error) => {
                self.emit_notes(vec![format!(
                    "the text beside the QR codes was not read: {}",
                    not_read(error)
                )]);
                (Passage::new(Vec::new(), Script::Spaced), None)
            }
        };
        if codes.is_empty() && passage.is_empty() {
            self.unread(Notice::NoTextFound, asked.shown);
            return;
        }
        let copied = if codes.is_empty() {
            passage.text(self.text_layout)
        } else {
            qr::copied(&codes)
        };
        let found = Found {
            passage,
            turn,
            codes,
        };
        let contents = text_contents(&copied);
        self.place_on_clipboard(ticket, contents, Copied::ReadText { asked, found }, None);
    }

    fn placed_codes(&self, codes: Result<Looked, String>, shown: Rect) -> Vec<Code> {
        match codes {
            Ok(looked) => {
                let enlarged = looked
                    .scan
                    .enlarged
                    .map_or_else(String::new, |by| format!(", enlarged {by:.1}x"));
                self.emit_notes(vec![format!(
                    "QR codes looked for in {} ms{enlarged}: {} read",
                    looked.took.as_millis(),
                    looked.scan.codes.len(),
                )]);
                looked.scan.placed(shown)
            }
            Err(reason) => {
                self.emit_notes(vec![format!("QR codes not looked for: {reason}")]);
                Vec::new()
            }
        }
    }

    fn placed_words(
        &self,
        reading: Reading,
        shown: Rect,
        codes: &[Code],
    ) -> (Passage, Option<Turn>) {
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
                        bounds: placement.on_screen(word.bounds, shown),
                        text: word.text,
                    })
                    .collect()
            })
            .collect();
        let turn = placement.turn(shown);
        let lines = qr::without_codes(lines, codes, turn);
        (Passage::new(lines, Script::of(&reading.language)), turn)
    }

    pub(super) fn text_copied(&mut self, asked: Asked, found: Found) {
        let said = qr::notice(&found.codes);
        let near = asked.shown;
        self.reselect(|session, _| session.show_words(asked.content, found));
        match said {
            Some(notice) => self.notify_at_once(vec![notice], near),
            None => self.tell_text_copied(),
        }
    }

    pub(super) fn text_not_copied(&mut self, error: CopyError, asked: &Asked) {
        self.notify_at_once(vec![Notice::CopyFailed(error)], asked.shown);
    }

    pub(super) fn copy_words(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(near) = session.selected() else {
            return;
        };
        let codes = session.codes_to_copy();
        let copy = match qr::notice(codes) {
            Some(said) => Some((qr::copied(codes), said)),
            None => session
                .words_text(self.text_layout)
                .map(|text| (text, Notice::TextCopied)),
        };
        let Some((text, said)) = copy else {
            return;
        };
        self.place_words(&text, near, said);
    }

    pub(super) fn lay_out_text(&mut self, layout: Layout) {
        self.text_layout = layout;
        (self.sink)(Event::TextLayout(layout));
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if !session.codes_to_copy().is_empty() {
            return;
        }
        let (Some(text), Some(near)) = (session.words_text(layout), session.selected()) else {
            return;
        };
        self.place_words(&text, near, Notice::TextCopied);
    }

    fn place_words(&mut self, text: &str, near: Rect, said: Notice) {
        let ticket = self.copies.reserve();
        let copied = Copied::Words { near, said };
        self.place_on_clipboard(ticket, text_contents(text), copied, None);
    }

    pub(super) fn words_copied(&mut self, near: Rect, said: Notice, result: Result<(), CopyError>) {
        let notice = match result {
            Ok(()) => said,
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

fn not_read(error: ReadError) -> Notice {
    match error {
        ReadError::NoLanguage => Notice::NoTextLanguage,
        ReadError::LanguageMissing => Notice::TextLanguageMissing,
        ReadError::Failed(reason) => Notice::TextNotRead(reason),
        ReadError::NeedsRestart(reason) => Notice::TextNeedsRestart(reason),
    }
}
