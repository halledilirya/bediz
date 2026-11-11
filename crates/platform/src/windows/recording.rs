pub mod border;
pub mod controls;
mod pipeline;
mod pointer;
mod ring;
mod scaler;
mod timer;

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bediz_core::animation::AnimationError;
use bediz_core::capture::Pixels;
use bediz_core::cursor::Placement;
use bediz_core::delivery::SaveError;
use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use bediz_core::recording::{self, FRAME_RATE, Pauses, RecordingStop, TIME_LIMIT};
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, USER_DEFAULT_SCREEN_DPI};

use super::clock::{Clock, Tick};
use super::foreground;
use super::memory;
use super::save;
use super::scene::Scene;
use crate::event::MenuTexts;
use border::{Border, Line};
use controls::{Controls, Stage};
use pipeline::Output;
pub use pipeline::{Ended, Pipeline};
use pointer::{Pointer, Sample};
use ring::Ring;
use scaler::Scaler;
pub use timer::Timer;

pub struct Request<'a> {
    pub instance: HINSTANCE,
    pub window: HWND,
    pub previous: HWND,
    pub monitor: usize,
    pub monitor_rect: Rect,
    pub area: Rect,
    pub folder: Option<&'a Path>,
    pub stem: &'a str,
    pub texts: &'a MenuTexts,
    pub first_capture: Duration,
    pub moved_bar: Option<Rect>,
}

pub enum StartError {
    Capture(windows::core::Error),
    Save(SaveError),
}

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
}

impl StartError {
    pub fn notice(self) -> Notice {
        match self {
            Self::Capture(error) => Notice::CaptureFailed(error.to_string()),
            Self::Save(error) => Notice::SaveFailed(error),
        }
    }
}

impl Recording {
    pub fn start(scene: &mut Scene, request: &Request) -> Result<Self, StartError> {
        let area = request.area;
        let (width, height) = recording::output_size(area.width(), area.height());
        let placement = Placement::new(area, width, height);
        let mut pointer = Pointer::default();
        let drawn = visible(
            pointer.sample(),
            scene.pointer_apart(request.monitor),
            &placement,
        );
        let (scaler, ring) = {
            let gpu = scene
                .gpu_for(request.monitor)
                .map_err(StartError::Capture)?;
            let relative = area.translated(-request.monitor_rect.left, -request.monitor_rect.top);
            let scaler =
                Scaler::new(&gpu.device, relative, width, height).map_err(StartError::Capture)?;
            let mut ring = Ring::new(gpu, width, height).map_err(StartError::Capture)?;
            if let Some(frozen) = scene.image_of(request.monitor) {
                scaler
                    .draw(&gpu.context, frozen, Pixels::Bgra8)
                    .map_err(StartError::Capture)?;
                ring.submit(&gpu.context, scaler.target(), Duration::ZERO, drawn.clone());
            }
            (scaler, ring)
        };
        let (dpi, work) = monitor_metrics(area);
        let scale = dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32;
        let border = Border::create(
            request.instance,
            area,
            request.monitor_rect,
            scale,
            Line::Solid,
        )
        .map_err(StartError::Capture)?;
        let controls = Controls::create(
            request.instance,
            Stage::Recording,
            area,
            work,
            dpi,
            request.texts,
            request.moved_bar,
        )
        .map_err(StartError::Capture)?;
        let folder = save::folder(request.folder).map_err(StartError::Save)?;
        let (pipeline, workers) =
            open_pipeline(&folder, request.stem, (width, height), request.window)
                .map_err(StartError::Save)?;
        let timer = Timer::start(request.first_capture, recording::frame_interval())
            .map_err(StartError::Capture)?;
        scene.begin_recording();
        let clock = Clock::new();
        Ok(Self {
            monitor: request.monitor,
            area,
            size: (width, height),
            folder,
            window: request.window,
            previous: request.previous,
            scaler,
            ring,
            pointer,
            placement,
            drawn,
            pipeline,
            timer: Some(timer),
            border,
            controls,
            started: clock.now(),
            clock,
            pauses: Pauses::default(),
            shown: 0,
            summary: format!(
                "recording a {}x{} area as {width}x{height} at {FRAME_RATE} fps, {workers}",
                area.width(),
                area.height(),
            ),
        })
    }

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
            Some(
                Timer::start(Duration::ZERO, recording::frame_interval())
                    .map_err(StartError::Capture)?,
            )
        };
        let (pipeline, workers) =
            open_pipeline(&self.folder, stem, self.size, self.window).map_err(StartError::Save)?;
        if timer.is_some() {
            self.timer = timer;
        }
        let previous = std::mem::replace(&mut self.pipeline, pipeline);
        previous.cancel();
        self.ring.discard();
        self.drawn = visible(
            self.pointer.sample(),
            scene.pointer_apart(self.monitor),
            &self.placement,
        );
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
            self.timer = Some(Timer::start(Duration::ZERO, recording::frame_interval())?);
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

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn tick(&mut self, scene: &mut Scene, notes: &mut Vec<String>) -> Next {
        let elapsed = self.elapsed();
        if let Err(stop) = self.collect(scene, false, notes) {
            return Next::Stop(stop);
        }
        if self.pipeline.failed() {
            return Next::Stop(None);
        }
        if elapsed >= TIME_LIMIT {
            return Next::Stop(Some(RecordingStop::TimeLimit(TIME_LIMIT)));
        }
        let sample = self.pointer.sample();
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

fn open_pipeline(
    folder: &Path,
    stem: &str,
    (width, height): (u32, u32),
    window: HWND,
) -> Result<(Pipeline, String), SaveError> {
    let (file, temporary) = save::create_recording(folder, stem)?;
    let threads = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let preparing = recording::preparing_threads(threads);
    let budget = recording::budget(memory::available_physical());
    let pipeline = Pipeline::start(
        width,
        height,
        budget,
        preparing,
        Output {
            file,
            temporary,
            folder: folder.to_path_buf(),
            stem: stem.to_owned(),
        },
        window,
    )?;
    let workers = format!(
        "{preparing} of {threads} threads preparing frames, a budget of {} MB",
        budget / 1_000_000
    );
    Ok((pipeline, workers))
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

pub fn monitor_metrics(area: Rect) -> (u32, Rect) {
    let half = |side: u32| i32::try_from(side / 2).unwrap_or(0);
    let centre = POINT {
        x: area.left + half(area.width()),
        y: area.top + half(area.height()),
    };
    let monitor = unsafe { MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let work = if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let work = info.rcWork;
        Rect::new(work.left, work.top, work.right, work.bottom)
    } else {
        area
    };
    let (mut dpi, mut unused) = (0, 0);
    let dpi = match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) } {
        Ok(()) if dpi > 0 => dpi,
        _ => USER_DEFAULT_SCREEN_DPI,
    };
    (dpi, work)
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
