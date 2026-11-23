//! What the screenshot and GIF shortcuts do when pressed: open the overlay, act as the key they are
//! inside it or a pin, or stop a recording, and report what a press tells of the key's registration.

use std::time::Duration;

use bediz_core::geometry::Point;
use windows::Win32::Foundation::{LPARAM, POINT};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::Runtime;
use crate::event::{Event, Shortcut};
use crate::windows::hotkey;
use crate::windows::session::Purpose;

impl Runtime {
    pub(super) fn on_hotkey(&mut self, keys: LPARAM, opens_after: Option<Duration>) {
        if self.recording_hotkey {
            (self.sink)(Event::HotkeyWhileRecording(Shortcut::Region));
        } else if self.recording.is_some() {
            self.stop_recording(None);
        } else if self.session.is_some() {
            if !self.locked() {
                self.press_in_overlay(keys);
            }
        } else if !self.press_in_pin(keys)
            && let Some(queued) = opens_after
        {
            self.open(queued, Purpose::Screenshot);
        }
        let (change, proves_yielding) = self.hotkey.arrived();
        self.report_hotkey(change);
        if proves_yielding {
            (self.sink)(Event::PrintScreenYields);
        }
    }

    pub(super) fn on_gif_hotkey(&mut self, keys: LPARAM, opens_after: Option<Duration>) {
        if self.recording_hotkey {
            (self.sink)(Event::HotkeyWhileRecording(Shortcut::Gif));
        } else if self.recording.is_some() {
            self.stop_recording(None);
        } else if let Some(session) = &self.session {
            if session.settled().is_some() {
                self.record(Duration::ZERO);
            }
        } else if !self.press_in_pin(keys)
            && let Some(queued) = opens_after
            && !self.closing()
        {
            self.open(queued, Purpose::Recording);
        }
        let (change, proves_yielding) = self.gif_hotkey.arrived();
        self.report_gif_hotkey(change);
        if proves_yielding {
            (self.sink)(Event::PrintScreenYields);
        }
    }

    // The shortcut is taken from every window, the overlay too, so there it acts as the key it is.
    fn press_in_overlay(&mut self, keys: LPARAM) {
        let mut cursor = POINT::default();
        let _ = unsafe { GetCursorPos(&mut cursor) };
        let (Some(scene), Some(session)) = (self.scene.as_ref(), self.session.as_mut()) else {
            return;
        };
        let windows = self.detect_windows.then_some(&self.windows);
        let action = session.press_key(
            scene,
            windows,
            self.last_region,
            hotkey::as_key_press(keys),
            Point::new(cursor.x, cursor.y),
        );
        self.act(action);
        self.follow_selection();
    }
}
