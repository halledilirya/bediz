use std::time::Duration;

use bediz_core::capture::Allocation;
use bediz_core::geometry::{Point, Rect};
use bediz_core::measurement::Measurement;
use bediz_core::notice::Notice;
use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, KillTimer, MSG, SPI_GETSELECTIONFADE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetTimer, SystemParametersInfoW,
};
use windows::core::{BOOL, Error, Result};

use super::{MENU_FADE_TIMER, Runtime};
use crate::event::{Event, Failure, Shortcut};
use crate::windows::foreground::{self, Foreground};
use crate::windows::hotkey;
use crate::windows::overlay_menu::{self, Choice, Offer};
use crate::windows::recording::Timer;
use crate::windows::scene::Scene;
use crate::windows::session::{Action, Purpose, Session};
use bediz_core::recording;

const MENU_FADE_MILLISECONDS: u32 = 500;

impl Runtime {
    pub(super) fn open(&mut self, queued: Duration, purpose: Purpose) {
        let start = self.clock.now();
        if std::mem::take(&mut self.menu_capture) {
            let _ = unsafe { KillTimer(Some(self.window.0), MENU_FADE_TIMER) };
        }
        if self.session.is_some() || self.held || self.recording.is_some() {
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
        if let Some(desktops) = &self.desktops {
            match desktops.bring_here(&scene.windows(), previous) {
                Ok(true) => {
                    notes.push("the overlay moved to the current virtual desktop".to_owned())
                }
                Ok(false) => {}
                Err(error) => notes.push(format!(
                    "the overlay's virtual desktop could not be checked: {error}"
                )),
            }
        }
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
        self.session = Some(Session::new(previous, self.enter, purpose));
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
        match foreground {
            Foreground::Taken => {}
            Foreground::TakenAfterInput => {
                notes.push("the overlay got the foreground only after an empty input".to_owned());
            }
            Foreground::Refused => {
                notes.push(
                    "the overlay did not get the foreground; a click on it takes it".to_owned(),
                );
            }
        }
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

    fn capture_failed(&self, reason: String) {
        (self.sink)(Event::Notice {
            notices: vec![Notice::CaptureFailed(reason)],
            near: pointer_rect(),
        });
    }

    // A failed draw may have lost the device, and only a new scene brings the overlay back.
    pub(super) fn draw_failed(&mut self, error: &Error) {
        (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        self.hide();
        self.rebuild_scene();
    }

    // The clicked menu item fades out for a moment and would be in the capture.
    pub(super) fn open_after_menu(&mut self) {
        let timer = menu_fades()
            && unsafe {
                SetTimer(
                    Some(self.window.0),
                    MENU_FADE_TIMER,
                    MENU_FADE_MILLISECONDS,
                    None,
                )
            } != 0;
        if timer {
            self.menu_capture = true;
        } else {
            self.open(Duration::ZERO, Purpose::Screenshot);
        }
    }

    pub(super) fn on_live_tick(&mut self) {
        let (Some(scene), Some(session)) = (self.scene.as_mut(), self.session.as_ref()) else {
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
        if let Err(error) = session.repaint(scene, &changed) {
            self.draw_failed(&error);
        }
    }

    pub(super) fn on_input(&mut self, message: &MSG) {
        let (Some(scene), Some(session)) = (self.scene.as_ref(), self.session.as_mut()) else {
            return;
        };
        let windows = self.detect_windows.then_some(&self.windows);
        let action = session.handle(scene, windows, self.last_region, message);
        self.act(action);
        self.place_ready_bar();
    }

    fn act(&mut self, action: Result<Action>) {
        match action {
            Ok(Action::None) => {}
            Ok(Action::Cancel) => self.hide(),
            Ok(Action::Deliver(delivery)) => self.deliver(delivery),
            Ok(Action::SaveAs) => self.save_as(),
            Ok(Action::Record) => self.record(Duration::ZERO),
            Ok(Action::CopyColour(at)) => self.copy_colour(at),
            Ok(Action::Menu(at)) => self.open_menu(at),
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
        } else if let Some(queued) = opens_after {
            self.open(queued, Purpose::Screenshot);
        }
        let (change, proves_yielding) = self.hotkey.arrived();
        self.report_hotkey(change);
        if proves_yielding {
            (self.sink)(Event::PrintScreenYields);
        }
    }

    pub(super) fn on_gif_hotkey(&mut self, queued: Duration) {
        if self.recording_hotkey {
            (self.sink)(Event::HotkeyWhileRecording(Shortcut::Gif));
        } else if self.recording.is_some() {
            self.stop_recording(None);
        } else if let Some(session) = &self.session {
            if session.settled().is_some() {
                self.record(Duration::ZERO);
            }
        } else {
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
        self.place_ready_bar();
    }

    fn open_menu(&mut self, at: Point) {
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
        let offer = Offer {
            selected: session.selected().is_some(),
            last_region: self.last_region.is_some(),
            enter: self.enter,
            recording: session.purpose() == Purpose::Recording,
        };
        match overlay_menu::choose(owner, at, texts, &offer) {
            Ok(Some(Choice::Deliver(delivery))) => self.deliver(delivery),
            Ok(Some(Choice::SaveAs)) => self.save_as(),
            Ok(Some(Choice::Record)) => self.record(menu_fade()),
            Ok(Some(Choice::CopyColour)) => self.copy_colour(at),
            Ok(Some(Choice::WholeMonitor)) => {
                self.reselect(|session, scene| session.select_whole_monitor(scene, at));
            }
            Ok(Some(Choice::LastRegion)) => {
                if let Some(region) = self.last_region {
                    self.reselect(|session, scene| session.select_region(scene, region));
                }
            }
            Ok(Some(Choice::Cancel)) => self.hide(),
            Ok(None) => {}
            Err(error) => (self.sink)(Event::Failed(Failure::Overlay(error.to_string()))),
        }
    }

    fn reselect(&mut self, change: impl FnOnce(&mut Session, &Scene)) {
        let (Some(scene), Some(session)) = (self.scene.as_ref(), self.session.as_mut()) else {
            return;
        };
        change(session, scene);
        let windows = self.detect_windows.then_some(&self.windows);
        if let Err(error) = session.redraw(scene, windows) {
            self.draw_failed(&error);
        }
        self.place_ready_bar();
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
        if session.purpose() == Purpose::Recording
            && let Some(scene) = self.scene.as_ref()
            && let Err(error) = scene.exclude_from_capture(false)
        {
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        }
        let _ = unsafe { ReleaseCapture() };
        if let Some(scene) = self.scene.as_ref()
            && let Err(error) = scene.hide()
        {
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        }
        // A locked region left the screen to the user, so the app they are in keeps the foreground.
        if give_back && locked {
            foreground::give_back_from_us(session.previous());
        } else if give_back {
            foreground::give_back(session.previous());
        }
        self.rest_scene();
        self.report_idle();
        if let Some(label) = self.label_after_overlay.take() {
            self.show_label(label);
        }
        session.previous()
    }

    fn rest_scene(&mut self) {
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

pub(super) fn menu_fade() -> Duration {
    if menu_fades() {
        Duration::from_millis(u64::from(MENU_FADE_MILLISECONDS))
    } else {
        Duration::ZERO
    }
}

fn menu_fades() -> bool {
    let mut fades = BOOL(0);
    unsafe {
        SystemParametersInfoW(
            SPI_GETSELECTIONFADE,
            0,
            Some((&mut fades as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok_and(|()| fades.as_bool())
}

fn pointer_rect() -> Rect {
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return Rect::default();
    }
    let at = Point::new(cursor.x, cursor.y);
    Rect::spanning(at, at)
}
