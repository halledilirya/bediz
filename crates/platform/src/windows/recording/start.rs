//! Starts a recording of an area: the frame and controls around it, its GPU resources, its file,
//! writing threads, timer and countdown; and the DPI and work area of the monitor an area is on.

use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Duration;

use bediz_core::capture::Pixels;
use bediz_core::cursor::Placement;
use bediz_core::delivery::SaveError;
use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use bediz_core::recording::{self, Pauses, RecordingOptions};
use windows::Win32::Foundation::{HINSTANCE, HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::USER_DEFAULT_SCREEN_DPI;

use super::border::{Border, Line};
use super::controls::{Controls, Stage};
use super::countdown::Countdown;
use super::pipeline::Output;
use super::pointer::Pointer;
use super::ring::Ring;
use super::scaler::Scaler;
use super::{Pipeline, Recording, Timer, visible};
use crate::event::MenuTexts;
use crate::windows::clock::Clock;
use crate::windows::contrast::Colours;
use crate::windows::memory;
use crate::windows::save;
use crate::windows::scene::Scene;

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
    pub options: RecordingOptions,
    pub colours: Option<Colours>,
}

pub enum StartError {
    Capture(windows::core::Error),
    Save(SaveError),
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
        let options = request.options;
        let (width, height) =
            recording::output_size(area.width(), area.height(), options.longest_side);
        let placement = Placement::new(area, width, height);
        let mut pointer = Pointer::default();
        let drawn = if options.cursor {
            visible(
                pointer.sample(),
                scene.pointer_apart(request.monitor),
                &placement,
            )
        } else {
            None
        };
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
                if !options.countdown {
                    ring.submit(&gpu.context, scaler.target(), Duration::ZERO, drawn.clone());
                }
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
        let interval = options.frame_rate.interval();
        let timer = Timer::start(request.first_capture, interval).map_err(StartError::Capture)?;
        let mut summary = format!(
            "recording a {}x{} area as {width}x{height} at {} fps, {workers}",
            area.width(),
            area.height(),
            options.frame_rate.per_second(),
        );
        let countdown = if options.countdown {
            match Countdown::open(request.instance, area, request.colours) {
                Ok(countdown) => Some(countdown),
                Err(error) => {
                    summary.push_str(&format!("; the countdown could not be shown: {error}"));
                    None
                }
            }
        } else {
            None
        };
        scene.begin_recording();
        let clock = Clock::new();
        let mut started = Self {
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
            summary,
            interval,
            time_limit: options.time_limit.duration(),
            cursor: options.cursor,
            countdown: None,
        };
        // The first frame waited for a countdown that could not be shown.
        if options.countdown && countdown.is_none() {
            started.begin(scene);
        }
        started.countdown = countdown;
        Ok(started)
    }
}

pub(super) fn open_pipeline(
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
