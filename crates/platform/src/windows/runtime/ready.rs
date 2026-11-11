use bediz_core::geometry::Rect;
use bediz_core::recording;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, USER_DEFAULT_SCREEN_DPI};

use super::Runtime;
use crate::windows::foreground::{self, Foreground};
use crate::windows::recording::border::{Border, Line};
use crate::windows::recording::controls::{Controls, Stage};
use crate::windows::recording::{Timer, monitor_metrics};
use crate::windows::session::{Purpose, Session};

pub(super) struct ReadyBar {
    controls: Controls,
    placed: Option<Rect>,
    lock: Option<Border>,
}

impl ReadyBar {
    pub(super) fn owns(&self, window: HWND) -> bool {
        self.controls.owns(window)
    }

    pub(super) fn is_locked(&self) -> bool {
        self.lock.is_some()
    }

    pub(super) fn moved(&self) -> Option<Rect> {
        self.controls.moved()
    }
}

impl Runtime {
    pub(super) fn locked(&self) -> bool {
        self.ready.as_ref().is_some_and(ReadyBar::is_locked)
    }

    pub(super) fn place_ready_bar(&mut self) {
        let area = self
            .session
            .as_ref()
            .filter(|session| session.purpose() == Purpose::Recording)
            .and_then(Session::settled)
            .zip(self.scene.as_ref())
            .and_then(|(selection, scene)| {
                recording::area_on_one_monitor(selection, scene.monitor_rects())
            })
            .map(|(_, area)| area);
        let Some(area) = area else {
            if let Some(bar) = self.ready.as_mut()
                && bar.placed.take().is_some()
            {
                bar.controls.hide();
            }
            return;
        };
        if self
            .ready
            .as_ref()
            .is_some_and(|bar| bar.placed == Some(area))
        {
            return;
        }
        let Some(texts) = self.menu_texts.as_ref() else {
            return;
        };
        let (dpi, work) = monitor_metrics(area);
        if self
            .ready
            .as_ref()
            .is_none_or(|bar| bar.controls.dpi() != dpi)
        {
            self.ready = None;
            match Controls::create(self.instance, Stage::Ready, area, work, dpi, texts, None) {
                Ok(created) => {
                    self.ready = Some(ReadyBar {
                        controls: created,
                        placed: None,
                        lock: None,
                    });
                }
                Err(error) => {
                    self.emit_notes(vec![format!(
                        "the buttons to start a recording could not be shown: {error}"
                    )]);
                    return;
                }
            }
        }
        let Some(bar) = self.ready.as_mut() else {
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
                "the buttons to start a recording could not be moved: {error}"
            )]),
        }
    }

    pub(super) fn lock_area(&mut self) {
        let (Some(bar), Some(scene), Some(session)) = (
            self.ready.as_mut(),
            self.scene.as_ref(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let Some(area) = bar.placed.filter(|_| bar.lock.is_none()) else {
            return;
        };
        let monitors = scene.monitor_rects();
        let Some(monitor) = recording::area_on_one_monitor(area, monitors)
            .and_then(|(index, _)| monitors.get(index).copied())
        else {
            return;
        };
        let (dpi, _) = monitor_metrics(area);
        let scale = dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32;
        let border = Border::create(self.instance, area, monitor, scale, Line::Dashed)
            .and_then(|border| border.reveal().map(|()| border));
        let border = match border {
            Ok(border) => border,
            Err(error) => {
                self.emit_notes(vec![format!(
                    "the region could not be locked, its frame did not appear: {error}"
                )]);
                return;
            }
        };
        if let Err(error) = scene.hide() {
            self.draw_failed(&error);
            return;
        }
        bar.lock = Some(border);
        bar.controls.show_toggled(true);
        foreground::give_back(session.previous());
        self.live = None;
    }

    pub(super) fn remember_users_window(&mut self) {
        let current = unsafe { GetForegroundWindow() };
        let ours = current.is_invalid()
            || self.is_overlay(current)
            || self.ready.as_ref().is_some_and(|bar| bar.owns(current));
        if let Some(session) = self.session.as_mut().filter(|_| !ours) {
            session.set_previous(current);
        }
    }

    pub(super) fn unlock_area(&mut self) {
        let (Some(bar), Some(scene), Some(session)) = (
            self.ready.as_mut(),
            self.scene.as_mut(),
            self.session.as_mut(),
        ) else {
            return;
        };
        if bar.lock.take().is_none() {
            return;
        }
        bar.controls.show_toggled(false);
        let mut notes = Vec::new();
        if let Err(error) = scene.refresh_live() {
            notes.push(format!(
                "the selection for a recording shows the screen as it was when locked: {error}"
            ));
        }
        if self.detect_windows {
            self.windows.refresh();
        }
        let windows = self.detect_windows.then_some(&self.windows);
        let drawn = scene
            .refresh_scales()
            .and_then(|()| scene.place())
            .and_then(|()| session.redraw_all(scene, windows));
        if let (Ok(()), Some(desktops)) = (&drawn, &self.desktops) {
            match desktops.bring_here(&scene.windows(), session.previous()) {
                Ok(true) => {
                    notes.push("the selection moved to the current virtual desktop".to_owned())
                }
                Ok(false) => {}
                Err(error) => notes.push(format!(
                    "the selection's virtual desktop could not be checked: {error}"
                )),
            }
        }
        let shown = drawn.and_then(|()| scene.uncloak());
        if let Err(error) = shown {
            self.emit_notes(notes);
            self.draw_failed(&error);
            return;
        }
        if scene.focus_under_cursor() == Foreground::Refused {
            notes.push(
                "the selection did not get the foreground; a click on it takes it".to_owned(),
            );
        }
        // Taking the foreground lifts the selection's window above every other topmost one.
        if let Err(error) = bar.controls.reveal() {
            notes.push(format!(
                "the buttons to start a recording may be under the selection: {error}"
            ));
        }
        match Timer::start(recording::live_interval(), recording::live_interval()) {
            Ok(timer) => self.live = Some(timer),
            Err(error) => notes.push(format!(
                "the selection for a recording shows a still screen: {error}"
            )),
        }
        self.emit_notes(notes);
    }
}
