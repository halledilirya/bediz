pub mod border;
pub mod controls;
mod countdown;
mod pipeline;
mod pointer;
mod ring;
mod scaler;
mod start;
mod timer;

use std::path::PathBuf;
use std::time::Duration;

use bediz_core::animation::AnimationError;
use bediz_core::cursor::Placement;
use bediz_core::geometry::Rect;
use bediz_core::recording::{self, COUNTDOWN, Pauses, RecordingStop};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use super::clock::{Clock, Tick};
use super::foreground;
use super::scene::Scene;
use border::Border;
use controls::Controls;
use countdown::Countdown;
pub use pipeline::{Ended, Pipeline};
use pointer::{Pointer, Sample};
use ring::Ring;
use scaler::Scaler;
use start::open_pipeline;
pub use start::{Request, StartError, monitor_metrics};
pub use timer::Timer;

pub enum Next {
    Going,
    Stop(Option<RecordingStop>),
}

pub struct Recording {
    monitor: usize,
    area: Rect,
    size: (u32, u32),
    folder: PathBuf,
    window: HWND,
    previous: HWND,
    scaler: Scaler,
    ring: Ring,
    pointer: Pointer,
    placement: Placement,
    drawn: Option<Sample>,
    pipeline: Pipeline,
    timer: Option<Timer>,
    border: Border,
    controls: Controls,
    clock: Clock,
    started: Tick,
    pauses: Pauses,
    shown: u64,
    summary: String,
    interval: Duration,
    time_limit: Duration,
    cursor: bool,
    countdown: Option<Countdown>,
}

impl Recording {
    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn restart(&mut self, scene: &Scene, stem: &str) -> Result<(Pipeline, String), StartError> {
        let timer = if self.timer.is_some() {
            None
        } else {
            Some(Timer::start(Duration::ZERO, self.interval).map_err(StartError::Capture)?)
        };
        let (pipeline, workers) =
            open_pipeline(&self.folder, stem, self.size, self.window).map_err(StartError::Save)?;
        if timer.is_some() {
            self.timer = timer;
        }
        let previous = std::mem::replace(&mut self.pipeline, pipeline);
        previous.cancel();
        self.ring.discard();
        self.countdown = None;
        self.begin(scene);
        self.controls.show_toggled(false);
        let mut note = format!("the recording started over, {workers}");
        if let Err(error) = self
            .controls
            .show_time(&recording::clock_text(Duration::ZERO))
        {
            note.push_str(&format!("; its clock could not be reset: {error}"));
        }
        Ok((previous, note))
    }

    pub fn set_paused(&mut self, paused: bool) -> windows::core::Result<()> {
        if paused == self.pauses.is_paused() {
            return Ok(());
        }
        let now = self.clock.since(self.started);
        if paused {
            self.timer = None;
            self.pauses.pause(now);
        } else {
            self.timer = Some(Timer::start(Duration::ZERO, self.interval)?);
            self.pauses.resume(now);
        }
        self.controls.show_toggled(paused);
        Ok(())
    }

    pub fn reveal(&self) -> windows::core::Result<()> {
        self.border.reveal()?;
        self.controls.reveal()
    }

    pub fn timer(&self) -> Option<HANDLE> {
        self.timer.as_ref().map(Timer::handle)
    }

    pub fn failed(&self) -> bool {
        self.pipeline.failed()
    }

    pub fn is_paused(&self) -> bool {
        self.pauses.is_paused()
    }

    pub fn counting_down(&self) -> bool {
        self.countdown.is_some()
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn tick(&mut self, scene: &mut Scene, notes: &mut Vec<String>) -> Next {
        let elapsed = self.elapsed();
        if self.countdown.is_some() {
            return self.count_down(scene, elapsed, notes);
        }
        if let Err(stop) = self.collect(scene, false, notes) {
            return Next::Stop(stop);
        }
        if self.pipeline.failed() {
            return Next::Stop(None);
        }
        if elapsed >= self.time_limit {
            return Next::Stop(Some(RecordingStop::TimeLimit(self.time_limit)));
        }
        let sample = if self.cursor {
            self.pointer.sample()
        } else {
            None
        };
        let scaler = &self.scaler;
        let grabbed = scene.grab(self.monitor, |gpu, frame, pixels| {
            scaler.draw(&gpu.context, frame, pixels)
        });
        let pointer = visible(sample, scene.pointer_apart(self.monitor), &self.placement);
        match grabbed {
            // A pointer that moved over a still screen still needs a frame of its own.
            Ok(fresh) if fresh || !same(&pointer, &self.drawn) => {
                if let Ok(gpu) = scene.gpu_for(self.monitor) {
                    self.ring
                        .submit(&gpu.context, self.scaler.target(), elapsed, pointer.clone());
                    self.drawn = pointer;
                }
            }
            Ok(_) => {}
            Err(error) => {
                notes.push(format!(
                    "the recording stopped; the screen could not be captured: {error}"
                ));
                return Next::Stop(Some(RecordingStop::Display));
            }
        }
        let seconds = elapsed.as_secs();
        if seconds != self.shown {
            self.shown = seconds;
            if let Err(error) = self.controls.show_time(&recording::clock_text(elapsed)) {
                notes.push(format!(
                    "the recording's clock could not be updated: {error}"
                ));
            }
        }
        Next::Going
    }

    pub fn stop(mut self, scene: Option<&mut Scene>, notes: &mut Vec<String>) -> (Pipeline, Rect) {
        let end = self.elapsed();
        if let Some(scene) = scene {
            if let Err(stop) = self.collect(scene, true, notes) {
                notes.push(format!(
                    "the last frames of the recording were not kept: {}",
                    stop.map_or_else(
                        || "they could not be stored".to_owned(),
                        |stop| stop.to_string()
                    )
                ));
            }
            scene.end_recording(self.monitor);
        }
        self.pipeline.finish(end);
        self.close()
    }

    pub fn cancel(self, scene: Option<&mut Scene>) -> (Pipeline, Rect) {
        if let Some(scene) = scene {
            scene.end_recording(self.monitor);
        }
        self.pipeline.cancel();
        self.close()
    }

    // A click on a button may have made the controls the foreground; it goes back to the recorded app.
    fn close(self) -> (Pipeline, Rect) {
        let focused = unsafe { GetForegroundWindow() } == self.controls.window();
        let Self {
            pipeline,
            area,
            previous,
            border,
            controls,
            ..
        } = self;
        drop(controls);
        drop(border);
        if focused {
            foreground::give_back(previous);
        }
        (pipeline, area)
    }

    fn elapsed(&self) -> Duration {
        self.pauses.recorded(self.clock.since(self.started))
    }

    fn begin(&mut self, scene: &Scene) {
        self.drawn = if self.cursor {
            visible(
                self.pointer.sample(),
                scene.pointer_apart(self.monitor),
                &self.placement,
            )
        } else {
            None
        };
        if let Ok(gpu) = scene.gpu_for(self.monitor) {
            self.ring.submit(
                &gpu.context,
                self.scaler.target(),
                Duration::ZERO,
                self.drawn.clone(),
            );
        }
        self.started = self.clock.now();
        self.pauses = Pauses::default();
        self.shown = 0;
    }

    // The screen is still followed during the countdown, so the first frame is the one it ends on.
    fn count_down(
        &mut self,
        scene: &mut Scene,
        elapsed: Duration,
        notes: &mut Vec<String>,
    ) -> Next {
        let scaler = &self.scaler;
        if let Err(error) = scene.grab(self.monitor, |gpu, frame, pixels| {
            scaler.draw(&gpu.context, frame, pixels)
        }) {
            notes.push(format!(
                "the recording stopped; the screen could not be captured: {error}"
            ));
            return Next::Stop(Some(RecordingStop::Display));
        }
        let left = recording::countdown_number(COUNTDOWN.saturating_sub(elapsed));
        if left > 0 {
            if let Some(countdown) = &mut self.countdown
                && let Err(error) = countdown.show(left)
            {
                notes.push(format!("the countdown could not be updated: {error}"));
            }
            return Next::Going;
        }
        self.countdown = None;
        self.begin(scene);
        Next::Going
    }

    fn collect(
        &mut self,
        scene: &Scene,
        wait: bool,
        notes: &mut Vec<String>,
    ) -> Result<(), Option<RecordingStop>> {
        let Ok(gpu) = scene.gpu_for(self.monitor) else {
            return Err(Some(RecordingStop::Display));
        };
        let frames = self.ring.collect(&gpu.context, wait).map_err(|error| {
            notes.push(format!("a recorded frame could not be read back: {error}"));
            Some(RecordingStop::Display)
        })?;
        for (mut image, at, pointer) in frames {
            if let Some(pointer) = pointer {
                pointer.cursor.draw(&mut image, pointer.at, &self.placement);
            }
            match self.pipeline.push(&image, at) {
                Ok(()) => {}
                Err(AnimationError::Budget { .. }) => return Err(Some(RecordingStop::Memory)),
                Err(error) => {
                    notes.push(format!("a recorded frame could not be stored: {error}"));
                    return Err(None);
                }
            }
        }
        Ok(())
    }
}

fn visible(sample: Option<Sample>, apart: bool, placement: &Placement) -> Option<Sample> {
    sample.filter(|sample| apart && sample.cursor.shows_in(sample.at, placement))
}

fn same(sample: &Option<Sample>, other: &Option<Sample>) -> bool {
    match (sample, other) {
        (Some(sample), Some(other)) => sample.same_as(other),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use bediz_core::cursor::{Cursor, Placement};
    use bediz_core::geometry::{Point, Rect};
    use bediz_core::image::Image;

    use super::pointer::Sample;
    use super::visible;

    fn screen() -> Image {
        Image::from_bgra(4, 4, [100, 100, 100, 255].repeat(16)).unwrap()
    }

    fn recorded(mut image: Image, sample: &Sample, apart: bool, placement: &Placement) -> Image {
        if let Some(pointer) = visible(Some(sample.clone()), apart, placement) {
            pointer.cursor.draw(&mut image, pointer.at, placement);
        }
        image
    }

    #[test]
    fn the_pointer_is_drawn_once_whether_or_not_the_capture_holds_it() {
        let half_red = [0, 0, 255, 128].repeat(4);
        let cursor = Cursor::from_colour(2, 2, Point::new(0, 0), &half_red, &[]).unwrap();
        let sample = Sample {
            at: Point::new(11, 11),
            cursor: Rc::new(cursor),
        };
        let placement = Placement::new(Rect::new(10, 10, 14, 14), 4, 4);
        let mut once = screen();
        sample.cursor.draw(&mut once, sample.at, &placement);
        assert_ne!(once, screen());
        assert_eq!(recorded(once.clone(), &sample, false, &placement), once);
        assert_eq!(recorded(screen(), &sample, true, &placement), once);
    }
}
