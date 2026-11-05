mod duplication;
mod gdi;

use bediz_core::capture::CaptureMethod;
use bediz_core::measurement::MonitorCapture;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::core::{Error, Result};

use super::displays::{Displays, Monitor};
use super::overlay::Overlay;
use duplication::Duplicator;
use gdi::GdiSurface;

enum Source {
    Duplication(Duplicator),
    Gdi(GdiSurface),
}

pub struct Capturer {
    sources: Vec<Option<Source>>,
}

impl Capturer {
    pub fn new(displays: &Displays, notes: &mut Vec<String>) -> Self {
        let sources = displays
            .monitors
            .iter()
            .map(|monitor| source(displays, monitor, notes))
            .collect();
        Self { sources }
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
        let begun: Vec<Option<Result<()>>> = self
            .sources
            .iter_mut()
            .zip(&targets)
            .map(|(source, target)| match (source, target) {
                (Some(Source::Duplication(duplicator)), Some((context, image))) => {
                    Some(duplicator.begin(context, image))
                }
                _ => None,
            })
            .collect();
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
                    .finish(begun, context, image)
                    .map(|fresh| (CaptureMethod::Duplication, fresh)),
                (Some(Source::Gdi(surface)), _) => surface
                    .capture(monitor.rect, context, image)
                    .map(|()| (CaptureMethod::Gdi, true)),
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
                    .map(|()| (CaptureMethod::Gdi, true))
            });
            match outcome {
                Ok((method, fresh)) => captures.push(MonitorCapture {
                    width: monitor.rect.width(),
                    height: monitor.rect.height(),
                    method,
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
