//! Screen capture of every monitor into its overlay image and of recording frames, through Desktop
//! Duplication where a monitor allows it and GDI where it does not.

mod duplication;
mod gdi;
mod scrgb;
mod white_level;

use std::time::Instant;

use bediz_core::capture::{CaptureMethod, Pixels};
use bediz_core::measurement::MonitorCapture;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::core::{Error, Result};

use super::displays::{Displays, Monitor};
use super::gpu::Gpu;
use super::overlay::Overlay;
use duplication::Duplicator;
use gdi::GdiSurface;
use white_level::WhiteLevels;

enum Source {
    Duplication(Duplicator),
    Gdi(GdiSurface),
}

pub struct Capturer {
    sources: Vec<Option<Source>>,
    levels: WhiteLevels,
    scratch: Option<ID3D11Texture2D>,
}

impl Capturer {
    pub fn new(displays: &Displays, notes: &mut Vec<String>) -> Self {
        let sources = displays
            .monitors
            .iter()
            .map(|monitor| source(displays, monitor, notes))
            .collect();
        Self {
            sources,
            levels: WhiteLevels::default(),
            scratch: None,
        }
    }

    pub fn begin_recording(&mut self) {
        self.levels = WhiteLevels::default();
        self.scratch = None;
    }

    pub fn grab(
        &mut self,
        displays: &Displays,
        overlays: &[Overlay],
        monitor: usize,
        draw: impl FnOnce(&Gpu, &ID3D11Texture2D, Pixels) -> Result<()>,
    ) -> Result<bool> {
        let (Some(found), Some(source)) = (
            displays.monitors.get(monitor),
            self.sources.get_mut(monitor),
        ) else {
            return Err(Error::new(E_FAIL, "the recorded monitor is gone"));
        };
        let gpu = displays
            .gpus
            .get(found.gpu)
            .ok_or_else(|| Error::new(E_FAIL, "the monitor has no device"))?;
        match source {
            Some(Source::Duplication(duplicator)) => duplicator.next(
                &gpu.context,
                overlays.get(monitor).and_then(Overlay::image),
                &mut self.levels,
                |frame, pixels| draw(gpu, frame, pixels),
            ),
            Some(Source::Gdi(surface)) => {
                let scratch = match self.scratch.take() {
                    Some(scratch) => scratch,
                    None => gpu.image_texture(found.rect.width(), found.rect.height())?,
                };
                let scratch = self.scratch.insert(scratch);
                surface.capture(found.rect, &gpu.context, scratch)?;
                draw(gpu, scratch, Pixels::Bgra8).map(|()| true)
            }
            None => Err(Error::new(E_FAIL, "the monitor has no capture source")),
        }
    }

    pub fn refresh(&mut self, displays: &Displays, overlays: &[Overlay]) -> Result<Vec<usize>> {
        let mut changed = Vec::new();
        let parts = self
            .sources
            .iter_mut()
            .zip(&displays.monitors)
            .zip(overlays)
            .enumerate();
        for (index, ((source, monitor), overlay)) in parts {
            let (Some(gpu), Some(image)) = (displays.gpus.get(monitor.gpu), overlay.image()) else {
                continue;
            };
            let fresh = match source {
                Some(Source::Duplication(duplicator)) => {
                    duplicator.refresh(&gpu.context, image, &mut self.levels)?
                }
                Some(Source::Gdi(surface)) => {
                    surface.capture(monitor.rect, &gpu.context, image)?;
                    true
                }
                None => false,
            };
            if fresh {
                changed.push(index);
            }
        }
        Ok(changed)
    }

    pub fn pointer_apart(&self, monitor: usize) -> bool {
        match self.sources.get(monitor) {
            Some(Some(Source::Duplication(duplicator))) => duplicator.pointer_apart(),
            _ => true,
        }
    }

    pub fn end_recording(&mut self, monitor: usize) {
        self.scratch = None;
        if let Some(Some(Source::Duplication(duplicator))) = self.sources.get_mut(monitor) {
            duplicator.settle();
        }
    }

    pub fn capture(
        &mut self,
        displays: &Displays,
        overlays: &[Overlay],
        notes: &mut Vec<String>,
    ) -> Result<Vec<MonitorCapture>> {
        let targets: Vec<Option<(&ID3D11DeviceContext, &ID3D11Texture2D)>> = displays
            .monitors
            .iter()
            .zip(overlays)
            .map(|(monitor, overlay)| {
                Some((&displays.gpus.get(monitor.gpu)?.context, overlay.image()?))
            })
            .collect();
        let mut levels = WhiteLevels::default();
        let begun: Vec<Option<Result<()>>> = self
            .sources
            .iter_mut()
            .zip(&targets)
            .map(|(source, target)| match (source, target) {
                (Some(Source::Duplication(duplicator)), Some((context, image))) => {
                    Some(duplicator.begin(context, image, &mut levels))
                }
                _ => None,
            })
            .collect();
        let begun_at = Instant::now();
        let mut captures = Vec::with_capacity(self.sources.len());
        let mut failure = None;
        let parts = self
            .sources
            .iter_mut()
            .zip(begun)
            .zip(targets)
            .zip(&displays.monitors);
        for (((source, begun), target), monitor) in parts {
            let Some((context, image)) = target else {
                failure.get_or_insert_with(|| {
                    Error::new(E_FAIL, "a monitor has no image to capture into")
                });
                continue;
            };
            let outcome = match (source, begun) {
                (Some(Source::Duplication(duplicator)), Some(begun)) => duplicator
                    .finish(begun, begun_at, context, image, &mut levels)
                    .map(|(fresh, pixels)| (CaptureMethod::Duplication, pixels, fresh)),
                (Some(Source::Gdi(surface)), _) => surface
                    .capture(monitor.rect, context, image)
                    .map(|()| (CaptureMethod::Gdi, Pixels::Bgra8, true)),
                _ => Err(Error::new(E_FAIL, "the monitor has no capture source")),
            };
            let outcome = outcome.or_else(|error| {
                notes.push(format!(
                    "capture of a {}x{} monitor failed, using GDI: {error}",
                    monitor.rect.width(),
                    monitor.rect.height()
                ));
                GdiSurface::new(monitor.rect.width(), monitor.rect.height())
                    .and_then(|surface| surface.capture(monitor.rect, context, image))
                    .map(|()| (CaptureMethod::Gdi, Pixels::Bgra8, true))
            });
            match outcome {
                Ok((method, pixels, fresh)) => captures.push(MonitorCapture {
                    width: monitor.rect.width(),
                    height: monitor.rect.height(),
                    method,
                    pixels,
                    fresh,
                }),
                Err(error) => {
                    failure.get_or_insert_with(|| {
                        Error::new(
                            E_FAIL,
                            format!(
                                "a {}x{} monitor could not be captured, GDI failed as well: {error}",
                                monitor.rect.width(),
                                monitor.rect.height()
                            ),
                        )
                    });
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(captures),
        }
    }

    pub fn hold_frames(&mut self) {
        let since = Instant::now();
        for source in &mut self.sources {
            if let Some(Source::Duplication(duplicator)) = source {
                duplicator.hold(since);
            }
        }
    }

    pub fn rearm(&mut self) {
        for source in &mut self.sources {
            if let Some(Source::Duplication(duplicator)) = source {
                duplicator.rearm();
            }
        }
    }

    pub fn reopen(&mut self) {
        for source in &mut self.sources {
            if let Some(Source::Duplication(duplicator)) = source {
                duplicator.reopen_quietly();
            }
        }
    }

    pub fn forget_targets(&mut self) {
        for source in &mut self.sources {
            if let Some(Source::Duplication(duplicator)) = source {
                duplicator.forget_target();
            }
        }
    }
}

fn source(displays: &Displays, monitor: &Monitor, notes: &mut Vec<String>) -> Option<Source> {
    let duplicated = displays
        .gpus
        .get(monitor.gpu)
        .ok_or_else(|| Error::new(E_FAIL, "the monitor has no device"))
        .and_then(|gpu| {
            let output = monitor
                .output
                .as_ref()
                .ok_or_else(|| Error::new(E_FAIL, "the monitor cannot be duplicated"))?;
            Duplicator::open(output, &gpu.device)
        });
    let error = match duplicated {
        Ok(duplicator) => return Some(Source::Duplication(duplicator)),
        Err(error) => error,
    };
    let (width, height) = (monitor.rect.width(), monitor.rect.height());
    match GdiSurface::new(width, height) {
        Ok(surface) => {
            notes.push(format!(
                "duplication is not available on a {width}x{height} monitor, using GDI: {error}"
            ));
            Some(Source::Gdi(surface))
        }
        Err(gdi_error) => {
            notes.push(format!(
                "no capture source for a {width}x{height} monitor: {error}; GDI: {gdi_error}"
            ));
            None
        }
    }
}

fn copy_texture(
    context: &ID3D11DeviceContext,
    target: &ID3D11Texture2D,
    source: &ID3D11Texture2D,
) -> Result<()> {
    let mut target_description = D3D11_TEXTURE2D_DESC::default();
    let mut source_description = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        target.GetDesc(&mut target_description);
        source.GetDesc(&mut source_description);
    }
    let region = D3D11_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: target_description.Width.min(source_description.Width),
        bottom: target_description.Height.min(source_description.Height),
        back: 1,
    };
    if region.right == 0 || region.bottom == 0 {
        return Err(Error::new(E_FAIL, "the captured frame is empty"));
    }
    unsafe { context.CopySubresourceRegion(target, 0, 0, 0, 0, source, 0, Some(&region)) };
    Ok(())
}
