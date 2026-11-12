use std::path::Path;
use std::time::Duration;

use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use bediz_core::recording::{self, RecordingStop};

use super::Runtime;
use super::ready::ReadyBar;
use crate::event::{Event, GifEvent, RecordingAction, RecordingState};
use crate::windows::recording::controls::{self, Button};
use crate::windows::recording::{Ended, Next, Pipeline, Recording, Request, StartError};
use crate::windows::{clipboard, clock};

pub(super) struct Finishing {
    pipeline: Pipeline,
    stop: Option<RecordingStop>,
    near: Rect,
    gif: Option<u64>,
    copy: Option<(u64, u32)>,
    told: Option<u8>,
    discarded: bool,
}

impl Finishing {
    fn unwanted(pipeline: Pipeline, near: Rect) -> Self {
        Self {
            pipeline,
            stop: None,
            near,
            gif: None,
            copy: None,
            told: None,
            discarded: false,
        }
    }

    pub(super) fn written_share(&self) -> Option<(u64, u64)> {
        self.pipeline.written_share()
    }

    pub(super) fn discard(&mut self) {
        self.discarded = true;
        self.pipeline.cancel();
    }
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
                self.finishing.push(Finishing::unwanted(pipeline, near));
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
        if paused && let Some(scene) = self.scene.as_mut() {
            scene.hold_frames();
        }
        self.show_recording_state();
    }

    pub(super) fn record(&mut self, first_capture: Duration) {
        if self.closing() {
            return;
        }
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
        let options = self.recording_options;
        let (width, height) =
            recording::output_size(area.width(), area.height(), options.longest_side);
        let stem = self.file_name.stem(&clock::local_time(), width, height);
        let mut notes = Vec::new();
        // The selection's image becomes the first frame, and while locked it stopped following the screen.
        if locked && let Err(error) = scene.capture(&mut notes) {
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
            options,
            colours: self.colours,
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
            self.finishing.push(Finishing::unwanted(pipeline, near));
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

    // A recording finished during its countdown holds no frame yet, so it is dropped.
    pub(super) fn stop_recording(&mut self, stop: Option<RecordingStop>) {
        if self
            .recording
            .as_ref()
            .is_some_and(Recording::counting_down)
        {
            let near = self.recording.as_ref().map(Recording::area);
            self.cancel_recording();
            if let (Some(stop), Some(near)) = (stop, near) {
                self.notify(vec![Notice::RecordingStopped(stop)], near);
            }
            return;
        }
        let Some(recording) = self.recording.take() else {
            return;
        };
        let mut notes = Vec::new();
        let (pipeline, near) = recording.stop(self.scene.as_mut(), &mut notes);
        self.emit_notes(notes);
        let gif = (!self.quitting).then(|| {
            self.gifs = self.gifs.wrapping_add(1);
            self.gifs
        });
        if let Some(id) = gif {
            (self.sink)(Event::Gif(GifEvent::Saving { id, near }));
        }
        let copy = (self.recording_options.copy && !self.quitting)
            .then(|| (self.copies.reserve(), clipboard::changes()));
        self.finishing.push(Finishing {
            pipeline,
            stop,
            near,
            gif,
            copy,
            told: None,
            discarded: false,
        });
        self.show_recording_state();
        self.on_recording_ended();
    }

    pub(super) fn cancel_recording(&mut self) {
        let Some(recording) = self.recording.take() else {
            return;
        };
        let (pipeline, near) = recording.cancel(self.scene.as_mut());
        self.finishing.push(Finishing::unwanted(pipeline, near));
        self.show_recording_state();
        self.on_recording_ended();
    }

    // A paused recording has no tick to notice that its writer failed.
    pub(super) fn on_recording_ended(&mut self) {
        if self.recording.as_ref().is_some_and(Recording::failed) {
            self.stop_recording(None);
            return;
        }
        for mut finishing in std::mem::take(&mut self.finishing) {
            match finishing.pipeline.take_ended() {
                Some(ended) => {
                    let Finishing {
                        pipeline,
                        stop,
                        near,
                        gif,
                        copy,
                        discarded,
                        ..
                    } = finishing;
                    drop(pipeline);
                    self.report_recording(ended, stop, near, gif, copy, discarded);
                }
                None => {
                    let percent = finishing.pipeline.written_percent();
                    if let (Some(id), Some(percent)) = (finishing.gif, percent)
                        && finishing.told != Some(percent)
                    {
                        finishing.told = Some(percent);
                        (self.sink)(Event::Gif(GifEvent::Written { id, percent }));
                    }
                    self.finishing.push(finishing);
                }
            }
        }
    }

    // On quit a recording is stopped and kept, and every GIF still being written is finished.
    pub(super) fn finish_recordings(&mut self) {
        self.quitting = true;
        self.stop_recording(None);
        for finishing in std::mem::take(&mut self.finishing) {
            let Finishing {
                pipeline,
                stop,
                near,
                gif,
                copy,
                discarded,
                ..
            } = finishing;
            let ended = pipeline.wait();
            self.report_recording(ended, stop, near, gif, copy, discarded);
        }
        self.finish_deletions();
    }

    fn report_recording(
        &mut self,
        ended: Ended,
        stop: Option<RecordingStop>,
        near: Rect,
        gif: Option<u64>,
        copy: Option<(u64, u32)>,
        discarded: bool,
    ) {
        if let Some(path) = left_behind(&ended, discarded) {
            self.delete_discarded(path.to_path_buf(), near);
        }
        if discarded {
            return;
        }
        let mut notices: Vec<Notice> = stop.map(Notice::RecordingStopped).into_iter().collect();
        match ended {
            Ended::Saved(saved, pace) => {
                self.emit_notes(vec![pace]);
                let path = saved.path.clone();
                if let Some(id) = gif {
                    (self.sink)(Event::Gif(GifEvent::Saved { id, gif: saved }));
                }
                if let Some(copy) = copy.filter(|_| !self.quitting) {
                    self.copy_recorded_gif(copy, gif, &path, notices, near);
                    return;
                }
                notices.push(Notice::GifSaved);
            }
            Ended::Failed(error) => {
                if let Some(id) = gif {
                    let error = error.clone();
                    (self.sink)(Event::Gif(GifEvent::NotSaved { id, error }));
                }
                notices.push(Notice::SaveFailed(error));
                self.quit_failed();
            }
            Ended::Cancelled => return,
        }
        self.notify(notices, near);
    }
}

// The writer may have finished the GIF just as it was told to stop, and then it is deleted after all.
fn left_behind(ended: &Ended, discarded: bool) -> Option<&Path> {
    match ended {
        Ended::Saved(saved, _) if discarded => Some(&saved.path),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use bediz_core::delivery::SaveError;

    use super::{Ended, left_behind};
    use crate::event::SavedGif;

    fn saved() -> Ended {
        Ended::Saved(
            SavedGif {
                path: PathBuf::from(r"C:\GIFs\Bediz 2026-11-12 640x360.gif"),
                bytes: 1,
                length: Duration::from_secs(1),
                width: 640,
                height: 360,
            },
            String::new(),
        )
    }

    #[test]
    fn a_gif_finished_after_it_was_discarded_is_deleted() {
        assert_eq!(
            left_behind(&saved(), true),
            Some(std::path::Path::new(
                r"C:\GIFs\Bediz 2026-11-12 640x360.gif"
            ))
        );
    }

    #[test]
    fn only_a_discarded_gif_that_was_saved_leaves_a_file_to_delete() {
        assert_eq!(left_behind(&saved(), false), None);
        assert_eq!(left_behind(&Ended::Cancelled, true), None);
        assert_eq!(left_behind(&Ended::Failed(SaveError::DiskFull), true), None);
    }
}
