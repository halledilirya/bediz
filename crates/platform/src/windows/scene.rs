//! Every monitor's overlay with its display and capture, handled as one: capturing, drawing,
//! showing and hiding, reading a selection across monitors and freeing GPU memory.

use bediz_core::capture::{Allocation, Pixels};
use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use bediz_core::measurement::{Memory, MonitorCapture};
use windows::Win32::Foundation::{E_FAIL, HINSTANCE, HWND, POINT};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
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
        self.capturer.forget_targets();
        let released = every(self.overlays.iter_mut().map(Overlay::release));
        let freed = every(self.displays.gpus.iter().map(Gpu::free_released));
        released.and(freed)
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

    pub fn gpu_for(&self, monitor: usize) -> Result<&Gpu> {
        let found = self
            .displays
            .monitors
            .get(monitor)
            .ok_or_else(|| Error::new(E_FAIL, "the monitor is gone"))?;
        gpu(&self.displays, found.gpu)
    }

    pub fn image_of(&self, monitor: usize) -> Option<&ID3D11Texture2D> {
        self.overlays.get(monitor)?.image()
    }

    pub fn begin_recording(&mut self) {
        self.capturer.begin_recording();
    }

    pub fn exclude_from_capture(&self, exclude: bool) -> Result<()> {
        every(
            self.overlays
                .iter()
                .map(|overlay| overlay.exclude_from_capture(exclude)),
        )
    }

    pub fn refresh_live(&mut self) -> Result<Vec<usize>> {
        self.capturer.refresh(&self.displays, &self.overlays)
    }

    pub fn grab(
        &mut self,
        monitor: usize,
        draw: impl FnOnce(&Gpu, &ID3D11Texture2D, Pixels) -> Result<()>,
    ) -> Result<bool> {
        self.capturer
            .grab(&self.displays, &self.overlays, monitor, draw)
    }

    pub fn pointer_apart(&self, monitor: usize) -> bool {
        self.capturer.pointer_apart(monitor)
    }

    pub fn end_recording(&mut self, monitor: usize) {
        self.capturer.end_recording(monitor);
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
        every(self.overlays.iter().map(Overlay::hide))
    }

    pub fn hold_frames(&mut self) {
        self.capturer.hold_frames();
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

// Each one is tried even after a failure, so one stuck overlay does not keep the others on screen.
fn every(results: impl Iterator<Item = Result<()>>) -> Result<()> {
    let mut all = Ok(());
    for result in results {
        all = all.and(result);
    }
    all
}

fn gpu(displays: &Displays, index: usize) -> Result<&Gpu> {
    displays
        .gpus
        .get(index)
        .ok_or_else(|| Error::new(E_FAIL, "the monitor has no device"))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL};
    use windows::core::Error;

    use super::every;

    #[test]
    fn every_overlay_is_tried_and_the_first_failure_is_reported() {
        let tried = Cell::new(0);
        let results = [
            Ok(()),
            Err(Error::from(E_FAIL)),
            Err(Error::from(E_ACCESSDENIED)),
            Ok(()),
        ];
        let outcome = every(results.into_iter().inspect(|_| tried.set(tried.get() + 1)));
        assert_eq!(tried.get(), 4);
        assert_eq!(outcome.map_err(|error| error.code()), Err(E_FAIL));
        assert!(every([Ok(()), Ok(())].into_iter()).is_ok());
    }
}
