//! The pins: made from a selection as it would be copied, shown and hidden, kept as a PNG, laid out
//! again when the displays change and shown anew on new devices when theirs are lost.

mod acting;
mod keeper;
mod tray;

use std::sync::Arc;

use bediz_core::geometry::Rect;
use bediz_core::image::Image;
use bediz_core::notice::Notice;
use bediz_core::pin::{Change, Count, Shelf};
use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::core::{Error, Result};

use super::Runtime;
use crate::event::{Event, PinAction, PinTexts};
use crate::windows::pin::{Devices, Pin};
use crate::windows::worker::Pixels;
use keeper::{Keeper, Kept};

#[derive(Default)]
pub(super) struct Pins {
    shelf: Shelf,
    pins: Vec<Pin>,
    devices: Option<Devices>,
    keeper: Keeper,
    made: u64,
    in_captures: bool,
    held_back: Vec<PinAction>,
    waiting: Vec<u64>,
    told: Count,
}

impl Pins {
    pub(super) fn owns(&self, window: HWND) -> bool {
        self.pins.iter().any(|pin| pin.window() == window)
    }

    fn find(&self, id: u64) -> Option<&Pin> {
        self.pins.iter().find(|pin| pin.id() == id)
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Pin> {
        self.pins.iter_mut().find(|pin| pin.id() == id)
    }

    fn id_of(&self, window: HWND) -> Option<u64> {
        self.pins
            .iter()
            .find(|pin| pin.window() == window)
            .map(Pin::id)
    }

    fn with_any_pin<T>(
        &mut self,
        id: u64,
        act: impl FnOnce(&mut Pin, &Devices) -> Result<T>,
    ) -> Result<T> {
        let devices = devices(&mut self.devices)?;
        let pin = self
            .pins
            .iter_mut()
            .find(|pin| pin.id() == id)
            .ok_or_else(|| Error::new(E_FAIL, "the pin is gone"))?;
        act(pin, devices)
    }

    fn renew_lost_devices(&mut self) -> Result<bool> {
        if !self.devices.as_ref().is_some_and(Devices::is_lost) {
            return Ok(false);
        }
        // A window takes one composition at a time, so the old ones go before new devices.
        for pin in &mut self.pins {
            pin.forget_composition();
        }
        // The lost devices go even when new ones cannot be made, so no message tries again.
        self.devices = None;
        self.devices = Some(Devices::create()?);
        Ok(true)
    }

    // The devices go once no pin shows, so hidden and closed pins hold no graphics memory.
    fn rest(&mut self) {
        if self.shelf.count().shown == 0 {
            self.devices = None;
        }
    }

    pub(super) fn close_every_window(&mut self) {
        self.shelf = Shelf::default();
        self.pins.clear();
        self.devices = None;
        self.held_back.clear();
        self.waiting.clear();
    }
}

fn devices(slot: &mut Option<Devices>) -> Result<&Devices> {
    if slot.is_none() {
        *slot = Some(Devices::create()?);
    }
    slot.as_ref()
        .ok_or_else(|| Error::new(E_FAIL, "the pins' devices could not be made"))
}

impl Runtime {
    pub(super) fn pin_selection(&mut self) {
        let Some((rect, read)) = self.read_selection() else {
            return;
        };
        self.hide();
        match read {
            Ok(image) => self.make_pin(image, rect),
            Err(error) => self.notify(vec![Notice::PinFailed(error.to_string())], rect),
        }
    }

    fn make_pin(&mut self, image: Image, rect: Rect) {
        let title = self
            .menu_texts
            .as_ref()
            .map(|texts| texts.pins.title_of(image.width(), image.height()))
            .unwrap_or_default();
        let bytes = u64::from(image.width())
            .saturating_mul(u64::from(image.height()))
            .saturating_mul(4);
        let room = devices(&mut self.pins.devices).map(|devices| devices.has_room(bytes));
        match room {
            Ok(true) => {}
            Ok(false) => {
                self.pins.rest();
                self.notify(vec![Notice::PinNoRoom], rect);
                return;
            }
            Err(error) => {
                self.notify(vec![Notice::PinFailed(error.to_string())], rect);
                return;
            }
        }
        self.pins.made = self.pins.made.wrapping_add(1);
        let id = self.pins.made;
        let image = Arc::new(image);
        let made = Pin::create(
            self.instance,
            id,
            Arc::clone(&image),
            rect,
            &title,
            self.pins.in_captures,
        );
        match made {
            Ok(pin) => self.pins.pins.push(pin),
            Err(error) => {
                self.pins.rest();
                self.notify(vec![Notice::PinFailed(error.to_string())], rect);
                return;
            }
        }
        let changes = self.pins.shelf.add(id);
        self.apply_pins(changes);
        self.keep_png(id, image);
        if self.pins.shelf.is_shown(id) {
            let near = self.pins.find(id).map_or(rect, Pin::rect);
            self.notify(vec![Notice::Pinned], near);
        }
    }

    // A PNG holds a screenshot in a fraction of its pixels' memory, and saving needs it anyway.
    fn keep_png(&mut self, id: u64, image: Arc<Image>) {
        if let Err(error) = self.pins.keeper.pack(self.window.0, id, image) {
            self.emit_notes(vec![format!(
                "a pin keeps its pixels as they are, unpacked: {error}"
            )]);
        }
    }

    pub(super) fn on_pin_kept(&mut self) {
        let mut notes = Vec::new();
        let mut failed = Vec::new();
        let mut unpacked = false;
        for kept in self.pins.keeper.take() {
            match kept {
                Kept::Packed(id, Ok(png)) => {
                    if let Some(pin) = self.pins.find_mut(id) {
                        pin.keep(png);
                    }
                }
                Kept::Packed(id, Err(error)) => {
                    if self.pins.find(id).is_some() {
                        notes.push(format!(
                            "a pin keeps its pixels as they are, unpacked: {error}"
                        ));
                    }
                }
                Kept::Unpacked(id, Ok(image)) => {
                    unpacked = true;
                    let shown = self
                        .with_shown_pin(id, |pin, devices| pin.compose(devices, &image))
                        .and_then(|()| self.show_kept(id));
                    if let Err(error) = shown {
                        failed.push((id, error));
                    }
                }
                Kept::Unpacked(id, Err(error)) => {
                    if self.pins.shelf.is_shown(id) {
                        failed.push((id, Error::new(E_FAIL, error)));
                    }
                }
            }
        }
        self.emit_notes(notes);
        self.keep_hidden(failed);
        if unpacked {
            self.keep_frames_on_top();
        }
        self.pins.rest();
        self.tell_pins();
    }

    fn apply_pins(&mut self, changes: Vec<Change>) {
        if let Err(error) = self.recover_pins() {
            self.pin_trouble(&error);
        }
        let raises = changes
            .iter()
            .any(|change| matches!(change, Change::Show(_)));
        let mut failed = Vec::new();
        for change in changes {
            match change {
                Change::Show(id) => {
                    if let Err(error) = self.show_kept(id) {
                        failed.push((id, error));
                    }
                }
                Change::Hide(id) => self.hide_pin(id),
                Change::Drop(id) => {
                    // Dropped with its window, uncommitted, a pin keeps its graphics memory.
                    self.hide_pin(id);
                    self.pins.pins.retain(|pin| pin.id() != id);
                }
            }
        }
        self.keep_hidden(failed);
        if raises {
            self.keep_frames_on_top();
        }
        self.pins.rest();
        self.tell_pins();
    }

    // Under the overlay a pin's pixels still go to the card, so it shows at once after.
    fn show_kept(&mut self, id: u64) -> Result<()> {
        let kept = self
            .pins
            .find(id)
            .filter(|_| self.pins.shelf.is_shown(id))
            .map(|pin| (pin.is_composed(), pin.pixels()));
        match kept {
            Some((true, _)) if self.covers_screen() => {
                self.pins.wait(id);
                Ok(())
            }
            Some((true, _)) => self.pins.with_any_pin(id, Pin::show),
            Some((false, Pixels::Image(image))) => {
                self.pins
                    .with_any_pin(id, |pin, devices| pin.compose(devices, &image))?;
                self.show_kept(id)
            }
            Some((false, Pixels::Png(png))) => self
                .pins
                .keeper
                .unpack(self.window.0, id, png)
                .map_err(|error| Error::new(E_FAIL, error.to_string())),
            None => Ok(()),
        }
    }

    fn hide_pin(&mut self, id: u64) {
        if let Some(pin) = self.pins.find_mut(id) {
            pin.hide();
        }
        if let Some(Err(error)) = self.pins.devices.as_ref().map(Devices::commit) {
            self.emit_notes(vec![format!("a pin could not be hidden: {error}")]);
        }
    }

    // A pin that cannot be shown keeps its pixels and stays hidden, to be shown again later.
    fn keep_hidden(&mut self, failed: Vec<(u64, Error)>) {
        let Some(near) = failed
            .first()
            .map(|(id, _)| self.pins.find(*id).map(Pin::rect).unwrap_or_default())
        else {
            return;
        };
        let mut reasons = Vec::new();
        for (id, error) in failed {
            self.pins.shelf.hide(id);
            self.hide_pin(id);
            reasons.push(error.to_string());
        }
        reasons.dedup();
        self.notify(vec![Notice::PinNotShown(reasons.join("; "))], near);
    }

    // Every shown pin's composition lives on the pins' devices, so a lost one remakes them all.
    fn recover_pins(&mut self) -> Result<bool> {
        let renewed = match self.pins.renew_lost_devices() {
            Ok(renewed) => renewed,
            Err(error) => {
                // Shown without a composition, a pin is an invisible window that still takes clicks.
                let failed = self.pins.shelf.shown();
                self.keep_hidden(failed.into_iter().map(|id| (id, error.clone())).collect());
                self.tell_pins();
                return Err(error);
            }
        };
        if !renewed {
            return Ok(false);
        }
        let mut failed = Vec::new();
        for id in self.pins.shelf.shown() {
            if let Err(error) = self.show_kept(id) {
                failed.push((id, error));
            }
        }
        self.keep_hidden(failed);
        self.keep_frames_on_top();
        self.pins.rest();
        self.tell_pins();
        Ok(true)
    }

    // A hidden pin is left alone, so input still queued for it or a late reply never shows it.
    fn with_shown_pin<T: Default>(
        &mut self,
        id: u64,
        act: impl FnOnce(&mut Pin, &Devices) -> Result<T>,
    ) -> Result<T> {
        if !self.pins.shelf.is_shown(id) {
            return Ok(T::default());
        }
        match self.pins.with_any_pin(id, act) {
            Ok(done) => Ok(done),
            // Devices stay valid between paints, so only a failed act checks them; new ones reshow it.
            Err(_) if self.recover_pins()? => Ok(T::default()),
            Err(error) => Err(error),
        }
    }

    // A pin raised during a recording or a locked area would otherwise cover its frame and bar.
    fn keep_frames_on_top(&self) {
        let raised = [
            self.recording.as_ref().map(|recording| recording.reveal()),
            self.ready.as_ref().map(|bar| bar.raise()),
        ];
        let notes = raised
            .into_iter()
            .flatten()
            .filter_map(Result::err)
            .map(|error| format!("a recording's frame or bar may be under a pin: {error}"))
            .collect();
        self.emit_notes(notes);
    }

    pub(super) fn retitle_pins(&self, texts: &PinTexts) {
        let failed = self
            .pins
            .pins
            .iter()
            .filter_map(|pin| {
                let (width, height) = pin.size();
                pin.retitle(&texts.title_of(width, height)).err()
            })
            .map(|error| format!("a pin keeps its name in the earlier language: {error}"))
            .collect();
        self.emit_notes(failed);
    }

    fn tell_pins(&mut self) {
        let count = self.pins.shelf.count();
        if count != self.pins.told {
            self.pins.told = count;
            (self.sink)(Event::Pins(count));
        }
    }

    pub(super) fn show_pins_in_captures(&mut self, shown: bool) {
        self.pins.in_captures = shown;
        let failed = self
            .pins
            .pins
            .iter()
            .filter_map(|pin| pin.show_in_captures(shown).err())
            .map(|error| format!("a pin may still be kept out of captures, or in them: {error}"))
            .collect();
        self.emit_notes(failed);
    }

    pub(super) fn place_pins(&mut self) {
        self.for_shown_pins(Pin::follow_monitors);
    }

    pub(super) fn repaint_pins(&mut self) {
        self.for_shown_pins(|pin, devices| pin.repaint_border(devices));
    }

    fn for_shown_pins(&mut self, act: impl Fn(&mut Pin, &Devices) -> Result<()>) {
        for id in self.pins.shelf.shown() {
            if let Err(error) = self.with_shown_pin(id, &act) {
                self.pin_trouble(&error);
            }
        }
    }

    fn pin_trouble(&self, error: &Error) {
        self.emit_notes(vec![format!("a pin could not be drawn as asked: {error}")]);
    }
}
