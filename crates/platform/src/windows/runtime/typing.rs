//! The input method's side of typing a text on the overlay: its composition handed to the
//! session and cancelled once the text took it, the input method turned on only while a text is
//! typed, and the keyboard focus brought back to the text from the bar.

use bediz_core::geometry::Point;
use bediz_core::markup::shape;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;

use super::Runtime;
use crate::windows::ime;
use crate::windows::session::Session;

impl Runtime {
    pub(super) fn follow_input_method(&mut self) {
        if let Some((window, updates)) = ime::take() {
            self.reselect(|session, scene| session.hear_input_method(scene, window, updates));
        }
        if let Some(session) = self.session.as_mut()
            && session.marking_mut().take_settled()
        {
            ime::cancel();
        }
        let typing = self.session.as_ref().is_some_and(Session::is_typing);
        if let Some(scene) = self.scene.as_ref() {
            scene.allow_input_method(typing);
        }
    }

    pub(super) fn keep_typing_focus(&self) {
        let (Some(scene), Some(session)) = (self.scene.as_ref(), self.session.as_ref()) else {
            return;
        };
        let Some(mark) = session.marking().markup().editing() else {
            return;
        };
        let bounds = shape::bounds(mark);
        if let Some(window) = scene.window_at(Point::new(bounds.left, bounds.top)) {
            let _ = unsafe { SetFocus(Some(window)) };
            // The focus taken back from a bar activates the overlay, which then covers the bars.
            self.raise_bars();
        }
    }
}
