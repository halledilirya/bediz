//! The bar of marking-up tools beside a screenshot's or an opened picture's selection: placing it,
//! showing the chosen tool and colour, acting on its buttons, and copying, cutting and pasting the
//! text being typed.

use bediz_core::delivery::Delivery;
use bediz_core::geometry::{Point, Rect};
use bediz_core::markup::{Swatch, Tool};
use bediz_core::recording;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::Result;

use super::Runtime;
use crate::event::{Event, Failure};
use crate::windows::clipboard::{self, Placed};
use crate::windows::marking::Marking;
use crate::windows::overlay_menu;
use crate::windows::recording::controls::{Button, Controls, Stage};
use crate::windows::recording::monitor_metrics;
use crate::windows::session::Session;

pub(super) struct MarkupBar {
    controls: Controls,
    placed: Option<Rect>,
    shown: Option<(Option<Tool>, Swatch)>,
}

impl MarkupBar {
    pub(super) fn owns(&self, window: HWND) -> bool {
        self.controls.owns(window)
    }

    pub(super) fn raise(&self) -> Result<()> {
        match self.placed {
            Some(_) => self.controls.reveal(),
            None => Ok(()),
        }
    }
}

impl Runtime {
    pub(super) fn place_markup_bar(&mut self) {
        let area = self
            .session
            .as_ref()
            .filter(|session| session.purpose().is_still())
            .and_then(Session::settled)
            .zip(self.scene.as_ref())
            .and_then(|(selection, scene)| {
                recording::area_on_one_monitor(selection, scene.monitor_rects())
            })
            .map(|(_, area)| area);
        let Some(area) = area else {
            if let Some(bar) = self.markup_bar.as_mut()
                && bar.placed.take().is_some()
            {
                bar.controls.hide();
            }
            return;
        };
        if self
            .markup_bar
            .as_ref()
            .is_some_and(|bar| bar.placed == Some(area))
        {
            self.show_markup_choice();
            return;
        }
        let Some(texts) = self.menu_texts.as_ref() else {
            return;
        };
        let (dpi, work) = monitor_metrics(area);
        if self
            .markup_bar
            .as_ref()
            .is_none_or(|bar| !bar.controls.suits(dpi, work))
        {
            self.markup_bar = None;
            match Controls::create(self.instance, Stage::Markup, area, work, dpi, texts, None) {
                Ok(controls) => {
                    self.markup_bar = Some(MarkupBar {
                        controls,
                        placed: None,
                        shown: None,
                    });
                }
                Err(error) => {
                    self.emit_notes(vec![format!(
                        "the marking-up tools could not be shown: {error}"
                    )]);
                    return;
                }
            }
        }
        let Some(bar) = self.markup_bar.as_mut() else {
            return;
        };
        let stays = bar.controls.moved().is_some() && bar.controls.work() == work;
        let placed = if stays {
            bar.controls.reveal()
        } else {
            bar.controls.place(area, work)
        };
        match placed {
            Ok(()) => bar.placed = Some(area),
            Err(error) => self.emit_notes(vec![format!(
                "the marking-up tools could not be moved: {error}"
            )]),
        }
        self.show_markup_choice();
    }

    pub(super) fn on_markup_button(&mut self, button: Button) {
        if let Some(tool) = button.tool() {
            self.change_marks(|marking| marking.choose(tool));
            return;
        }
        match button {
            Button::Colour => self.pick_colour(),
            Button::Undo => self.change_marks(|marking| {
                marking.markup_mut().undo();
            }),
            Button::Redo => self.change_marks(|marking| {
                marking.markup_mut().redo();
            }),
            Button::Copy => self.deliver(Delivery::COPY),
            Button::Save => self.deliver(Delivery::SAVE),
            Button::ReadText => self.read_text(),
            _ => {}
        }
        self.keep_typing_focus();
    }

    pub(super) fn copy_text(&mut self, cut: bool) {
        let text = self
            .session
            .as_ref()
            .and_then(|session| session.marking().markup().selected_text())
            .map(str::to_owned);
        let Some(text) = text else {
            return;
        };
        match self.place_text(&text) {
            Ok(Placed::Done) if cut => {
                self.change_marks(|marking| marking.markup_mut().erase(false, false));
            }
            Ok(Placed::Done) => {}
            Ok(_) => self.emit_notes(vec![
                "the clipboard was busy; the text was not copied".to_owned(),
            ]),
            Err(error) => self.emit_notes(vec![format!("the text could not be copied: {error}")]),
        }
    }

    pub(super) fn paste_text(&mut self) {
        match clipboard::text(self.window.0) {
            Ok(Some(text)) => self.change_marks(|marking| marking.markup_mut().type_text(&text)),
            Ok(None) => {}
            Err(error) => self.emit_notes(vec![format!("the text could not be pasted: {error}")]),
        }
    }

    fn show_markup_choice(&mut self) {
        let (Some(bar), Some(session)) = (self.markup_bar.as_mut(), self.session.as_ref()) else {
            return;
        };
        let choice = (session.marking().tool(), session.marking().swatch());
        if bar.shown != Some(choice) {
            bar.controls.show_choice(choice.0, choice.1.colour());
            bar.shown = Some(choice);
        }
    }

    fn change_marks(&mut self, change: impl FnOnce(&mut Marking)) {
        self.reselect(|session, _| change(session.marking_mut()));
    }

    fn pick_colour(&mut self) {
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        let at = Point::new(cursor.x, cursor.y);
        let (Some(texts), Some(scene), Some(session)) = (
            self.menu_texts.as_ref(),
            self.scene.as_ref(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let Some(owner) = scene.window_at(at) else {
            return;
        };
        match overlay_menu::choose_colour(owner, at, texts, session.marking().swatch()) {
            Ok(Some(swatch)) => self.change_marks(|marking| marking.recolour(swatch)),
            Ok(None) => {}
            Err(error) => (self.sink)(Event::Failed(Failure::Overlay(error.to_string()))),
        }
    }
}
