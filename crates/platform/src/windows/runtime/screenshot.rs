//! Opens the overlay for a screenshot or a recording on a hotkey or the app's command, measures
//! how fast it appeared, acts on the keys and clicks made in it, and closes it.

use std::time::Duration;

use bediz_core::capture::Allocation;
use bediz_core::geometry::{Point, Rect};
use bediz_core::measurement::Measurement;
use bediz_core::notice::Notice;
use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetQueueStatus, KillTimer, MSG, QS_INPUT, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_RBUTTONUP,
};
use windows::core::{Error, Result};

use super::{MENU_FADE_TIMER, Runtime};
use crate::event::{Event, Failure, Shortcut};
use crate::windows::foreground::{self, Foreground};
use crate::windows::hotkey;
use crate::windows::label_window::LabelWindow;
use crate::windows::recording::Timer;
use crate::windows::scene::Scene;
use crate::windows::session::{Action, Purpose, Session};
use crate::windows::virtual_desktop::VirtualDesktops;
use bediz_core::recording;

impl Runtime {
    // A delay's countdown closes once the overlay shows, off the hotkey path; captures leave it out.
    pub(super) fn open(&mut self, queued: Duration, purpose: Purpose) {
        self.show_overlay(queued, purpose);
        self.cancel_delay();
    }

    fn show_overlay(&mut self, queued: Duration, purpose: Purpose) {
        let start = self.clock.now();
        if std::mem::take(&mut self.menu_capture) {
            let _ = unsafe { KillTimer(Some(self.window.0), MENU_FADE_TIMER) };
        }
        if !self.can_open() {
            return;
        }
        let mut notes = Vec::new();
        if self.displays_changed {
            self.rebuild_scene();
            notes.push("the displays had just changed; the overlay was rebuilt first".to_owned());
        } else if self.scene.is_none() {
            self.rebuild_scene();
            notes.push("no overlay was ready; it was built again first".to_owned());
        }
        let Some(scene) = self.scene.as_mut() else {
            self.emit_notes(notes);
            self.capture_failed("no display is ready".to_owned());
            return;
        };
        let previous = unsafe { GetForegroundWindow() };
        if let Err(error) = scene.allocate() {
            self.emit_notes(notes);
            self.capture_failed(format!(
                "the overlay's memory could not be allocated: {error}"
            ));
            self.rest_scene();
            return;
        }
        let monitors = match scene.capture(&mut notes) {
            Ok(monitors) => monitors,
            Err(error) => {
                self.emit_notes(notes);
                self.capture_failed(error.to_string());
                self.rest_scene();
                return;
            }
        };
        let captured = self.clock.since(start);
        let windows = if self.detect_windows {
            self.windows.refresh()
        } else {
            0
        };
        let listed = self.clock.since(start);
        let drawn = scene
            .refresh_scales()
            .and_then(|()| scene.place())
            .and_then(|()| scene.draw_all());
        let presented = self.clock.since(start);
        bring_here(self.desktops.as_ref(), scene, previous, &mut notes);
        let live = purpose == Purpose::Recording && {
            scene.begin_recording();
            match scene.exclude_from_capture(true) {
                Ok(()) => true,
                Err(error) => {
                    notes.push(format!(
                        "the selection for a recording shows a still screen: {error}"
                    ));
                    false
                }
            }
        };
        let shown = drawn.and_then(|()| scene.uncloak());
        let uncloaked = self.clock.since(start);
        let foreground = if shown.is_ok() {
            scene.focus_under_cursor()
        } else {
            Foreground::Refused
        };
        let focused = self.clock.since(start);
        self.session = Some(Session::new(
            previous,
            self.enter,
            purpose,
            scene,
            self.window.0,
        ));
        if let Err(error) = shown {
            self.emit_notes(notes);
            self.hide();
            self.build_scene();
            self.capture_failed(format!("the overlay could not be shown: {error}"));
            return;
        }
        if live {
            match Timer::start(recording::live_interval(), recording::live_interval()) {
                Ok(timer) => self.live = Some(timer),
                Err(error) => notes.push(format!(
                    "the selection for a recording shows a still screen: {error}"
                )),
            }
        }
        notes.extend(foreground_note(foreground));
        let first_view = match self.session.as_mut() {
            Some(session) => session.redraw(scene, self.detect_windows.then_some(&self.windows)),
            None => Ok(()),
        };
        let memory = scene.memory();
        self.emit_notes(notes);
        (self.sink)(Event::Measured(Measurement {
            allocation: self.allocation,
            monitors,
            queued,
            captured,
            listed,
            presented,
            uncloaked,
            focused,
            windows,
            memory,
        }));
        self.hide_label();
        if let Err(error) = first_view {
            self.draw_failed(&error);
        }
    }

    pub(super) fn can_open(&self) -> bool {
        self.session.is_none() && !self.held && self.recording.is_none()
    }

    pub(super) fn capture_failed(&self, reason: String) {
        (self.sink)(Event::Notice {
            notices: vec![Notice::CaptureFailed(reason)],
            near: pointer_rect(),
            at_once: false,
        });
    }

    // A failed draw may have lost the device, and only a new scene brings the overlay back.
    pub(super) fn draw_failed(&mut self, error: &Error) {
        (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        self.hide();
        self.rebuild_scene();
    }

    pub(super) fn on_live_tick(&mut self) {
        let (Some(scene), Some(session)) = (self.scene.as_mut(), self.session.as_mut()) else {
            self.live = None;
            return;
        };
        let changed = match scene.refresh_live() {
            Ok(changed) => changed,
            Err(error) => {
                self.live = None;
                self.emit_notes(vec![format!(
                    "the selection for a recording stopped following the screen: {error}"
                )]);
                return;
            }
        };
        let input_waiting = (unsafe { GetQueueStatus(QS_INPUT) } >> 16) & QS_INPUT.0 != 0;
        if let Err(error) = session.repaint(scene, &changed, input_waiting) {
            self.draw_failed(&error);
        }
    }

    pub(super) fn on_input(&mut self, message: &MSG) {
        let (Some(scene), Some(session)) = (self.scene.as_ref(), self.session.as_mut()) else {
            return;
        };
        let windows = self.detect_windows.then_some(&self.windows);
        let action = session.handle(scene, windows, self.last_region, message);
        // A press lifts the overlay above the bars and the label, which are topmost windows too.
        if matches!(
            message.message,
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONUP
        ) {
            self.raise_bars();
        }
        self.act(action);
        self.follow_selection();
    }

    pub(super) fn follow_selection(&mut self) {
        self.place_ready_bar();
        self.place_markup_bar();
        self.announce();
    }

    pub(super) fn raise_bars(&self) {
        let raised = [
            self.ready.as_ref().map(|bar| bar.raise()),
            self.markup_bar.as_ref().map(|bar| bar.raise()),
            self.label.as_ref().map(LabelWindow::raise),
        ];
        let notes = raised
            .into_iter()
            .flatten()
            .filter_map(Result::err)
            .map(|error| {
                format!("the bars or the label beside the selection may be under it: {error}")
            })
            .collect();
        self.emit_notes(notes);
    }

    fn act(&mut self, action: Result<Action>) {
        match action {
            Ok(Action::None) => {}
            Ok(Action::Cancel) => self.hide(),
            Ok(Action::Deliver(delivery)) => self.deliver(delivery),
            Ok(Action::SaveAs) => self.save_as(),
            Ok(Action::Pin) => self.pin_selection(),
            Ok(Action::Record) => self.record(Duration::ZERO),
            Ok(Action::CopyColour(at)) => self.copy_colour(at),
            Ok(Action::Menu(at)) => self.open_menu(at),
            Ok(Action::CopyText { cut }) => self.copy_text(cut),
            Ok(Action::PasteText) => self.paste_text(),
            Ok(Action::ReadText) => self.read_text(),
            Ok(Action::CopyWords) => self.copy_words(),
            Err(error) => self.draw_failed(&error),
        }
    }

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

    pub(super) fn reselect(&mut self, change: impl FnOnce(&mut Session, &Scene)) {
        let (Some(scene), Some(session)) = (self.scene.as_ref(), self.session.as_mut()) else {
            return;
        };
        change(session, scene);
        let windows = self.detect_windows.then_some(&self.windows);
        if let Err(error) = session.redraw(scene, windows) {
            self.draw_failed(&error);
        }
        self.follow_selection();
    }

    pub(super) fn hide(&mut self) {
        self.close_overlay(true);
    }

    pub(super) fn close_overlay(&mut self, give_back: bool) -> HWND {
        let Some(session) = self.session.take() else {
            return HWND::default();
        };
        let locked = self.locked();
        self.live = None;
        self.ready = None;
        self.markup_bar = None;
        self.heard = Default::default();
        self.turns.forget();
        if session.purpose().shows_live_screen()
            && let Some(scene) = self.scene.as_ref()
            && let Err(error) = scene.exclude_from_capture(false)
        {
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        }
        if session.purpose() == Purpose::Picture
            && let Some(scene) = self.scene.as_mut()
        {
            scene.forget_picture();
        }
        let _ = unsafe { ReleaseCapture() };
        if let Some(scene) = self.scene.as_ref()
            && let Err(error) = scene.hide()
        {
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        }
        let previous = session.previous();
        let trim =
            session.purpose() == Purpose::Picture || session.marking().markup().revision() != 0;
        // A locked region left the screen to the user, so the app they are in keeps the foreground.
        if give_back && locked {
            foreground::give_back_from_us(previous);
        } else if give_back {
            foreground::give_back(previous);
        }
        // The idle line measures what stays once the session's images, marks and history are gone.
        drop(session);
        self.rest_scene();
        // Only images and marks grow the buffers, and the next opening has to make them again.
        if trim
            && let Some(scene) = self.scene.as_ref()
            && let Err(error) = scene.trim()
        {
            self.emit_notes(vec![format!(
                "the graphics memory could not be trimmed: {error}"
            )]);
        }
        self.release_held_pins();
        self.report_idle();
        if let Some(label) = self.label_after_overlay.take() {
            self.show_label(label);
        }
        previous
    }

    pub(super) fn rest_scene(&mut self) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if self.allocation == Allocation::OnHotkey
            && let Err(error) = scene.release()
        {
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        }
        scene.rearm();
    }
}

pub(super) fn bring_here(
    desktops: Option<&VirtualDesktops>,
    scene: &Scene,
    previous: HWND,
    notes: &mut Vec<String>,
) {
    let Some(desktops) = desktops else {
        return;
    };
    match desktops.bring_here(&scene.windows(), previous) {
        Ok(true) => notes.push("the overlay moved to the current virtual desktop".to_owned()),
        Ok(false) => {}
        Err(error) => notes.push(format!(
            "the overlay's virtual desktop could not be checked: {error}"
        )),
    }
}

pub(super) fn foreground_note(foreground: Foreground) -> Option<String> {
    match foreground {
        Foreground::Taken => None,
        Foreground::TakenAfterInput => {
            Some("the overlay got the foreground only after an empty input".to_owned())
        }
        Foreground::Refused => {
            Some("the overlay did not get the foreground; a click on it takes it".to_owned())
        }
    }
}

pub(super) fn pointer() -> Option<Point> {
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }
        .ok()
        .map(|()| Point::new(cursor.x, cursor.y))
}

pub(super) fn pointer_rect() -> Rect {
    pointer().map_or_else(Rect::default, |at| Rect::spanning(at, at))
}
