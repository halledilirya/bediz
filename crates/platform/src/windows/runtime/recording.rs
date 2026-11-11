use std::time::Duration;

use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use bediz_core::recording::{self, RecordingStop};

use super::Runtime;
use super::ready::ReadyBar;
use crate::event::{Event, RecordingAction, RecordingState};
use crate::windows::clock;
use crate::windows::recording::controls::{self, Button};
use crate::windows::recording::{Ended, Next, Pipeline, Recording, Request, StartError};

pub(super) struct Finishing {
    pipeline: Pipeline,
    stop: Option<RecordingStop>,
    near: Rect,
}

impl Runtime {
    pub(super) fn on_controls(&mut self, request: controls::Request) {
        if self.locked() {
            self.remember_users_window();
        }
        match request {
            controls::Request::Press(Button::Start) => self.record(Duration::ZERO),
            controls::Request::Press(Button::Lock) => self.lock_area(),
            controls::Request::Press(Button::Unlock) => self.unlock_area(),
            controls::Request::Press(Button::Pause) => self.set_paused(true),
            controls::Request::Press(Button::Resume) => self.set_paused(false),
            controls::Request::Press(Button::Finish) => self.stop_recording(None),
            controls::Request::Press(Button::Restart) => self.restart_recording(),
            controls::Request::Press(Button::Discard) => self.cancel_recording(),
            controls::Request::Close if self.recording.is_some() => self.stop_recording(None),
            controls::Request::Press(Button::Cancel) | controls::Request::Close => self.hide(),
        }
    }

    pub(super) fn on_recording_action(&mut self, action: RecordingAction) {
        match action {
            RecordingAction::Pause => self.set_paused(true),
            RecordingAction::Resume => self.set_paused(false),
            RecordingAction::Finish => self.stop_recording(None),
            RecordingAction::Restart => self.restart_recording(),
            RecordingAction::Discard => self.cancel_recording(),
        }
    }

    fn show_recording_state(&self) {
        let state = match &self.recording {
            None => RecordingState::Off,
            Some(recording) if recording.is_paused() => RecordingState::Paused,
            Some(_) => RecordingState::Going,
        };
        (self.sink)(Event::Recording(state));
    }

    fn restart_recording(&mut self) {
        let (Some(recording), Some(scene)) = (self.recording.as_mut(), self.scene.as_ref()) else {
            return;
        };
        let near = recording.area();
        let (width, height) = recording.size();
        let stem = self.file_name.stem(&clock::local_time(), width, height);
        match recording.restart(scene, &stem) {
            Ok((pipeline, note)) => {
                self.emit_notes(vec![note]);
                self.finishing.push(Finishing {
                    pipeline,
                    stop: None,
                    near,
                });
                self.on_recording_ended();
                self.show_recording_state();
            }
            Err(error) => {
                self.cancel_recording();
                self.notify(vec![error.notice()], near);
            }
        }
    }

    fn set_paused(&mut self, paused: bool) {
        let Some(recording) = self.recording.as_mut() else {
            return;
        };
        if let Err(error) = recording.set_paused(paused) {
            self.emit_notes(vec![format!(
                "the recording could not go on after its pause, so it was finished: {error}"
            )]);
            self.stop_recording(None);
            return;
        }
        self.show_recording_state();
    }

    pub(super) fn record(&mut self, first_capture: Duration) {
        let locked = self.locked();
        let (Some(scene), Some(session), Some(texts)) = (
            self.scene.as_mut(),
            self.session.as_ref(),
            self.menu_texts.as_ref(),
        ) else {
            return;
        };
        let Some(selection) = session.selected() else {
            return;
        };
        let monitors = scene.monitor_rects();
        let Some((monitor, area)) = recording::area_on_one_monitor(selection, monitors) else {
            return;
        };
        let Some(monitor_rect) = monitors.get(monitor).copied() else {
            return;
        };
        let (width, height) = recording::output_size(area.width(), area.height());
        let stem = self.file_name.stem(&clock::local_time(), width, height);
        let mut notes = Vec::new();
        // The selection's image becomes the first frame, and while locked it stopped following the screen.
        if locked && let Err(error) = scene.refresh_live() {
            notes.push(format!(
                "the recording's first frame may show the screen as it was when locked: {error}"
            ));
        }
        let request = Request {
            instance: self.instance,
            window: self.window.0,
            previous: session.previous(),
            monitor,
            monitor_rect,
            area,
            folder: self.save_folder.as_deref(),
            stem: &stem,
            texts,
            first_capture,
            moved_bar: self.ready.as_ref().and_then(ReadyBar::moved),
        };
        let started = Recording::start(scene, &request);
        self.hide();
        self.emit_notes(notes);
        let recording = match started {
            Ok(recording) => recording,
            Err(error) => {
                self.notify(vec![error.notice()], area);
                return;
            }
        };
        if let Err(error) = recording.reveal() {
            let (pipeline, near) = recording.cancel(self.scene.as_mut());
            self.finishing.push(Finishing {
                pipeline,
                stop: None,
                near,
            });
            self.on_recording_ended();
            self.notify(vec![StartError::Capture(error).notice()], area);
            return;
        }
        self.emit_notes(vec![recording.summary().to_owned()]);
        self.recording = Some(recording);
        self.show_recording_state();
    }

    pub(super) fn on_recording_tick(&mut self) {
        let (Some(recording), Some(scene)) = (self.recording.as_mut(), self.scene.as_mut()) else {
            self.stop_recording(None);
            return;
        };
        let mut notes = Vec::new();
        let next = recording.tick(scene, &mut notes);
        self.emit_notes(notes);
        if let Next::Stop(stop) = next {
            self.stop_recording(stop);
        }
    }

    pub(super) fn stop_recording(&mut self, stop: Option<RecordingStop>) {
        let Some(recording) = self.recording.take() else {
            return;
        };
        let mut notes = Vec::new();
        let (pipeline, near) = recording.stop(self.scene.as_mut(), &mut notes);
        self.emit_notes(notes);
        self.finishing.push(Finishing {
            pipeline,
            stop,
            near,
        });
        self.show_recording_state();
        self.on_recording_ended();
    }

    pub(super) fn cancel_recording(&mut self) {
        let Some(recording) = self.recording.take() else {
            return;
        };
        let (pipeline, near) = recording.cancel(self.scene.as_mut());
        self.finishing.push(Finishing {
            pipeline,
            stop: None,
            near,
        });
        self.show_recording_state();
        self.on_recording_ended();
    }

    // A paused recording has no tick to notice that its writer failed.
    pub(super) fn on_recording_ended(&mut self) {
        if self.recording.as_ref().is_some_and(Recording::failed) {
            self.stop_recording(None);
            return;
        }
        for finishing in std::mem::take(&mut self.finishing) {
            match finishing.pipeline.take_ended() {
                Some(ended) => {
                    drop(finishing.pipeline);
                    self.report_recording(ended, finishing.stop, finishing.near);
                }
                None => self.finishing.push(finishing),
            }
        }
    }

    // On quit a recording is stopped and kept, and every GIF still being written is finished.
    pub(super) fn finish_recordings(&mut self) {
        self.stop_recording(None);
        for finishing in std::mem::take(&mut self.finishing) {
            let ended = finishing.pipeline.wait();
            self.report_recording(ended, finishing.stop, finishing.near);
        }
    }

    pub(super) fn recording_busy(&self) -> bool {
        self.recording.is_some() || !self.finishing.is_empty()
    }

    fn report_recording(&self, ended: Ended, stop: Option<RecordingStop>, near: Rect) {
        let mut notices: Vec<Notice> = stop.map(Notice::RecordingStopped).into_iter().collect();
        match ended {
            Ended::Saved => notices.push(Notice::GifSaved),
            Ended::Failed(error) => notices.push(Notice::SaveFailed(error)),
            Ended::Cancelled => return,
        }
        self.notify(notices, near);
    }
}
