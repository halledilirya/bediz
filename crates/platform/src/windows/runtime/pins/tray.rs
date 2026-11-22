//! The tray's commands for every pin, and the commands and pins that would show one held back
//! while the overlay covers the screen, until it closes.

use bediz_core::pin::Change;

use super::Pins;
use crate::event::PinAction;
use crate::windows::runtime::Runtime;

impl Pins {
    fn would_show(&self, action: PinAction) -> bool {
        let count = self.shelf.count();
        match action {
            PinAction::ShowOrHide => count.shown == 0 && count.open > 0,
            PinAction::Restore => count.restorable,
            PinAction::CloseAll => false,
        }
    }

    pub(super) fn wait(&mut self, id: u64) {
        if !self.waiting.contains(&id) {
            self.waiting.push(id);
        }
    }
}

impl Runtime {
    // A pin shown now would come up over the selection; a locked area leaves the screen free.
    pub(super) fn covers_screen(&self) -> bool {
        self.session.is_some() && !self.locked()
    }

    pub(crate) fn holds_pins_back(&self, action: PinAction) -> bool {
        self.covers_screen() && self.pins.would_show(action)
    }

    // The tray's item reads the same until the pins change, so a second click asks the same.
    pub(crate) fn hold_pins_back(&mut self, action: PinAction) {
        if !self.pins.held_back.contains(&action) {
            self.pins.held_back.push(action);
        }
    }

    pub(crate) fn release_held_pins(&mut self) {
        let waiting = std::mem::take(&mut self.pins.waiting);
        if !waiting.is_empty() {
            self.apply_pins(waiting.into_iter().map(Change::Show).collect());
        }
        self.let_held_actions_go();
    }

    // Held back only to show pins, a toggle let go after a restore would hide them instead.
    pub(crate) fn let_held_actions_go(&mut self) {
        for action in std::mem::take(&mut self.pins.held_back) {
            let changes = match action {
                PinAction::ShowOrHide => self.pins.shelf.show_all(),
                PinAction::Restore => self.pins.shelf.restore(),
                PinAction::CloseAll => self.pins.shelf.close_all(),
            };
            self.apply_pins(changes);
        }
    }

    pub(crate) fn on_pin_action(&mut self, action: PinAction) {
        let changes = match action {
            PinAction::ShowOrHide => self.pins.shelf.show_or_hide(),
            PinAction::Restore => self.pins.shelf.restore(),
            PinAction::CloseAll => self.pins.shelf.close_all(),
        };
        self.apply_pins(changes);
    }
}
