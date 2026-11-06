use bediz_core::capture::Allocation;
use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use bediz_core::measurement::{Memory, MonitorCapture};
use windows::Win32::Foundation::{E_FAIL, HINSTANCE, HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::{Error, Result};

use super::capture::Capturer;
use super::contrast::Colours;
use super::displays::Displays;
use super::foreground::{self, Foreground};
use super::gpu::Gpu;
use super::memory;
use super::overlay::{Overlay, View};
use super::readback;

const GRIP_REACH: f32 = 8.0;

pub struct Scene {
    overlays: Vec<Overlay>,
    capturer: Capturer,
    displays: Displays,
    rects: Vec<Rect>,
}

impl Scene {
    pub fn new(
        instance: HINSTANCE,
        allocation: Allocation,
        notes: &mut Vec<String>,
    ) -> Result<Self> {
        let displays = Displays::enumerate(notes)?;
        if displays.monitors.is_empty() {
            return Err(Error::new(E_FAIL, "no monitor is attached to the desktop"));
        }
        let mut overlays = Vec::with_capacity(displays.monitors.len());
        for monitor in &displays.monitors {
            let gpu = gpu(&displays, monitor.gpu)?;
            overlays.push(Overlay::create(
                instance,
                gpu,
                monitor.rect,
                allocation == Allocation::Ahead,
            )?);
        }
        let capturer = Capturer::new(&displays, notes);
        let rects = displays
            .monitors
            .iter()
            .map(|monitor| monitor.rect)
            .collect();
        let scene = Self {
            overlays,
            capturer,
            displays,
            rects,
        };
        if allocation == Allocation::Ahead {
            scene.warm_up()?;
        }
        Ok(scene)
    }

    pub fn monitor_rects(&self) -> &[Rect] {
        &self.rects
    }

    pub fn reach_at(&self, at: Point) -> i32 {
        let scale = self
            .overlays
            .iter()
            .find(|overlay| overlay.rect().contains(at))
            .map_or(1.0, Overlay::scale);
        (GRIP_REACH * scale).round() as i32
    }

    pub fn owns(&self, window: HWND) -> bool {
        self.overlays
            .iter()
            .any(|overlay| overlay.window() == window)
    }

    pub fn allocate(&mut self) -> Result<()> {
        for (overlay, monitor) in self.overlays.iter_mut().zip(&self.displays.monitors) {
            overlay.allocate(gpu(&self.displays, monitor.gpu)?)?;
        }
        Ok(())
    }

    // Allocations become resident on first use; drawing once keeps that cost off the hotkey.
    pub fn warm_up(&self) -> Result<()> {
        self.draw_all()
    }

    pub fn release(&mut self) -> Result<()> {
        for overlay in &mut self.overlays {
            overlay.release()?;
        }
        for gpu in &self.displays.gpus {
            gpu.free_released()?;
        }
        Ok(())
    }

    pub fn use_colours(&self, colours: Option<&Colours>) {
        for gpu in &self.displays.gpus {
            gpu.use_colours(colours);
        }
    }

    pub fn capture(&mut self, notes: &mut Vec<String>) -> Result<Vec<MonitorCapture>> {
        self.capturer.capture(&self.displays, &self.overlays, notes)
    }

    pub fn reopen_capture(&mut self) {
        self.capturer.reopen();
    }

    pub fn refresh_scales(&mut self) -> Result<()> {
        for (overlay, monitor) in self.overlays.iter_mut().zip(&self.displays.monitors) {
            overlay.refresh_scale(gpu(&self.displays, monitor.gpu)?)?;
        }
        Ok(())
    }

    pub fn place(&self) -> Result<()> {
        self.overlays.iter().try_for_each(Overlay::place)
    }

    pub fn draw_all(&self) -> Result<()> {
        for index in 0..self.overlays.len() {
            self.draw_one(index, &View::default())?;
        }
        Ok(())
    }

    pub fn draw_one(&self, monitor: usize, view: &View) -> Result<()> {
        let (Some(overlay), Some(found)) = (
            self.overlays.get(monitor),
            self.displays.monitors.get(monitor),
        ) else {
            return Ok(());
        };
        overlay.draw(gpu(&self.displays, found.gpu)?, view)
    }

    pub fn pixel(&self, at: Point) -> Option<[u8; 4]> {
        let (overlay, monitor) = self
            .overlays
            .iter()
            .zip(&self.displays.monitors)
            .find(|(overlay, _)| overlay.rect().contains(at))?;
        overlay.pixel(gpu(&self.displays, monitor.gpu).ok()?, at)
    }

    pub fn uncloak(&self) -> Result<()> {
        self.overlays.iter().try_for_each(Overlay::uncloak)
    }

    pub fn window_at(&self, at: Point) -> Option<HWND> {
        self.overlays
            .iter()
            .find(|overlay| overlay.rect().contains(at))
            .or_else(|| self.overlays.first())
            .map(Overlay::window)
    }

    pub fn windows(&self) -> Vec<HWND> {
        self.overlays.iter().map(Overlay::window).collect()
    }

    pub fn focus_under_cursor(&self) -> Foreground {
        let mut cursor = POINT::default();
        let point = match unsafe { GetCursorPos(&mut cursor) } {
            Ok(()) => Point::new(cursor.x, cursor.y),
            Err(_) => Point::default(),
        };
        let overlay = self
            .overlays
            .iter()
            .find(|overlay| overlay.rect().contains(point))
            .or_else(|| self.overlays.first());
        match overlay {
            Some(overlay) => foreground::take(overlay.window()),
            None => Foreground::Refused,
        }
    }

    pub fn hide(&self) -> Result<()> {
        self.overlays.iter().try_for_each(Overlay::hide)
    }

    pub fn rearm(&mut self) {
        self.capturer.rearm();
    }

    pub fn read(&self, rect: Rect) -> Result<Image> {
        let mut canvas = Image::blank(rect.width(), rect.height())
            .map_err(|error| Error::new(E_FAIL, error.to_string()))?;
        let mut covered = false;
        for (overlay, monitor) in self.overlays.iter().zip(&self.displays.monitors) {
            let Some(part) = rect.intersection(&monitor.rect) else {
                continue;
            };
            let image = overlay
                .image()
                .ok_or_else(|| Error::new(E_FAIL, "the overlay holds no image"))?;
            let pixels = readback::read(
                gpu(&self.displays, monitor.gpu)?,
                image,
                part.translated(-monitor.rect.left, -monitor.rect.top),
            )?;
            let (Ok(x), Ok(y)) = (
                u32::try_from(part.left - rect.left),
                u32::try_from(part.top - rect.top),
            ) else {
                continue;
            };
            canvas.paste_opaque(&pixels, x, y);
            covered = true;
        }
        if covered {
            Ok(canvas)
        } else {
            Err(Error::new(E_FAIL, "the selection is on no monitor"))
        }
    }

    pub fn memory(&self) -> Memory {
        memory::measure(self.displays.gpus.iter().map(|gpu| &gpu.adapter))
    }
}

fn gpu(displays: &Displays, index: usize) -> Result<&Gpu> {
    displays
        .gpus
        .get(index)
        .ok_or_else(|| Error::new(E_FAIL, "the monitor has no device"))
}
