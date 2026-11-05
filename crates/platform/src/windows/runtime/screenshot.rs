use std::time::Duration;

use bediz_core::capture::Allocation;
use bediz_core::geometry::Rect;
use bediz_core::measurement::Measurement;
use bediz_core::notice::Notice;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, KillTimer, MSG, SPI_GETSELECTIONFADE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetTimer, SystemParametersInfoW,
};
use windows::core::BOOL;

use super::{MENU_FADE_TIMER, Runtime};
use crate::event::{Event, Failure};
use crate::windows::foreground::{self, Foreground};
use crate::windows::session::{Action, Session};

const MENU_FADE_MILLISECONDS: u32 = 500;

impl Runtime {
    pub(super) fn open(&mut self, queued: Duration) {
        let start = self.clock.now();
        if std::mem::take(&mut self.menu_capture) {
            let _ = unsafe { KillTimer(Some(self.window.0), MENU_FADE_TIMER) };
        }
        if self.session.is_some() {
            return;
        }
        let mut notes = Vec::new();
        if self.displays_changed {
            self.rebuild_scene();
            notes.push("the displays had just changed; the overlay was rebuilt first".to_owned());
        }
        let Some(scene) = self.scene.as_mut() else {
            (self.sink)(Event::Failed(Failure::Displays(
                "no display is ready".to_owned(),
            )));
            return;
        };
        let previous = unsafe { GetForegroundWindow() };
        if let Err(error) = scene.allocate() {
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
            self.rest_scene();
            return;
        }
        let monitors = match scene.capture(&mut notes) {
            Ok(monitors) => monitors,
            Err(error) => {
                self.emit_notes(notes);
                (self.sink)(Event::Notice {
                    notices: vec![Notice::CaptureFailed(error.to_string())],
                    near: pointer_rect(),
                });
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
        let shown = drawn.and_then(|()| scene.uncloak());
        let uncloaked = self.clock.since(start);
        let foreground = if shown.is_ok() {
            scene.focus_under_cursor()
        } else {
            Foreground::Refused
        };
        let focused = self.clock.since(start);
        self.session = Some(Session::new(previous));
        if let Err(error) = shown {
            self.emit_notes(notes);
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
            self.hide();
            self.build_scene();
            return;
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
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.redraw(scene, self.detect_windows.then_some(&self.windows))
        {
            notes.push(format!("the magnifier could not be drawn: {error}"));
        }
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
    }

    // The clicked menu item fades out for a moment and would be in the capture.
    pub(super) fn open_after_menu(&mut self) {
        let mut fades = BOOL(0);
        let fade_on = unsafe {
            SystemParametersInfoW(
                SPI_GETSELECTIONFADE,
                0,
                Some((&mut fades as *mut BOOL).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .is_ok_and(|()| fades.as_bool());
        let timer = fade_on
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
            self.open(Duration::ZERO);
        }
    }

    pub(super) fn on_input(&mut self, message: &MSG) {
        let (Some(scene), Some(session)) = (self.scene.as_ref(), self.session.as_mut()) else {
            return;
        };
        let windows = self.detect_windows.then_some(&self.windows);
        match session.handle(scene, windows, self.last_region, message) {
            Ok(Action::None) => {}
            Ok(Action::Cancel) => self.hide(),
            Ok(Action::Deliver(delivery)) => self.deliver(delivery),
            Ok(Action::CopyColour(at)) => self.copy_colour(at),
            Err(error) => (self.sink)(Event::Failed(Failure::Overlay(error.to_string()))),
        }
    }

    pub(super) fn hide(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        let _ = unsafe { ReleaseCapture() };
        if let Some(scene) = self.scene.as_ref()
            && let Err(error) = scene.hide()
        {
            (self.sink)(Event::Failed(Failure::Overlay(error.to_string())));
        }
        foreground::give_back(session.previous());
        self.rest_scene();
        self.report_idle();
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

fn pointer_rect() -> Rect {
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return Rect::default();
    }
    Rect::new(
        cursor.x,
        cursor.y,
        cursor.x.saturating_add(1),
        cursor.y.saturating_add(1),
    )
}
