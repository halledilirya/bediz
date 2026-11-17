//! Marking up an image from a file or the clipboard: asks for it off this thread, then opens the
//! overlay on the live screen with the image as the selection, on the monitor under the pointer.

use std::sync::PoisonError;

use bediz_core::image::Image;
use bediz_core::notice::Notice;
use bediz_core::picture::OpenError;
use bediz_core::recording;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::Runtime;
use super::screenshot::{bring_here, foreground_note, pointer, pointer_rect};
use crate::event::{Event, ImageSource};
use crate::windows::foreground::Foreground;
use crate::windows::picture::{self, Source};
use crate::windows::recording::Timer;
use crate::windows::session::{Picture, Session};

impl Runtime {
    pub(super) fn mark_up(&mut self, from: ImageSource) {
        if self.opening_picture.is_some() || self.recording.is_some() || self.closing() {
            return;
        }
        self.pictures_asked = self.pictures_asked.wrapping_add(1);
        let source = match from {
            ImageSource::Clipboard => Source::Clipboard,
            ImageSource::File => Source::File {
                kind: self
                    .menu_texts
                    .as_ref()
                    .map(|texts| texts.images.clone())
                    .unwrap_or_default(),
                usual_folder: self.save_folder.clone(),
            },
        };
        match picture::open(self.window.0, &self.pictures, self.pictures_asked, source) {
            Ok(()) => self.opening_picture = Some(self.pictures_asked),
            Err(error) => self.picture_failed(OpenError::Unexpected(format!(
                "the thread that opens images could not start: {error}"
            ))),
        }
    }

    pub(super) fn on_picture(&mut self) {
        let opened = self
            .pictures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(opened) = opened.filter(|opened| Some(opened.request) == self.opening_picture)
        else {
            return;
        };
        self.opening_picture = None;
        match opened.image {
            Some(Ok(image)) => self.open_picture(image),
            Some(Err(error)) => self.picture_failed(error),
            None => {}
        }
    }

    fn open_picture(&mut self, image: Image) {
        if self.session.is_some() || self.held || self.recording.is_some() || self.closing() {
            self.emit_notes(vec![
                "an image was opened while Bediz was busy, so it was not shown".to_owned(),
            ]);
            return;
        }
        let mut notes = Vec::new();
        if self.displays_changed || self.scene.is_none() {
            self.rebuild_scene();
        }
        let Some(scene) = self.scene.as_mut() else {
            self.picture_failed(OpenError::Unexpected("no display is ready".to_owned()));
            return;
        };
        let previous = unsafe { GetForegroundWindow() };
        let at = pointer().unwrap_or_default();
        let placed = scene
            .allocate()
            .and_then(|()| scene.capture(&mut notes).map(drop))
            .and_then(|()| scene.refresh_scales())
            .and_then(|()| scene.show_picture(&image, at));
        let shown = match placed {
            Ok(shown) => shown,
            Err(error) => {
                scene.forget_picture();
                self.emit_notes(notes);
                self.rest_scene();
                self.picture_failed(OpenError::Unexpected(format!(
                    "the image could not be shown: {error}"
                )));
                return;
            }
        };
        let drawn = scene.place().and_then(|()| scene.draw_all());
        bring_here(self.desktops.as_ref(), scene, previous, &mut notes);
        scene.begin_recording();
        let live = match scene.exclude_from_capture(true) {
            Ok(()) => true,
            Err(error) => {
                notes.push(format!("the image is shown over a still screen: {error}"));
                false
            }
        };
        let uncloaked = drawn.and_then(|()| scene.uncloak());
        let foreground = if uncloaked.is_ok() {
            scene.focus_under_cursor()
        } else {
            Foreground::Refused
        };
        let picture = Picture { image, shown };
        let session = Session::for_picture(previous, self.enter, scene, self.window.0, picture);
        let session = self.session.insert(session);
        if let Err(error) = uncloaked {
            self.emit_notes(notes);
            self.hide();
            self.build_scene();
            self.picture_failed(OpenError::Unexpected(format!(
                "the overlay could not be shown: {error}"
            )));
            return;
        }
        notes.extend(foreground_note(foreground));
        if live {
            match Timer::start(recording::live_interval(), recording::live_interval()) {
                Ok(timer) => self.live = Some(timer),
                Err(error) => {
                    notes.push(format!("the image is shown over a still screen: {error}"))
                }
            }
        }
        let first_view = session.redraw(scene, None);
        self.emit_notes(notes);
        self.hide_label();
        match first_view {
            Ok(()) => self.follow_selection(),
            Err(error) => self.draw_failed(&error),
        }
    }

    fn picture_failed(&self, error: OpenError) {
        (self.sink)(Event::Notice {
            notices: vec![Notice::ImageNotOpened(error)],
            near: pointer_rect(),
        });
    }
}
