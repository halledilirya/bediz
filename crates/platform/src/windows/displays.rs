//! Finds the monitors attached to the desktop and creates a device for each graphics adapter that
//! drives one.

use bediz_core::geometry::Rect;
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Factory1,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIAdapter1, IDXGIFactory2, IDXGIOutput,
};
use windows::core::Result;

use super::gpu::Gpu;

pub struct Monitor {
    pub rect: Rect,
    pub gpu: usize,
    pub output: Option<IDXGIOutput>,
}

pub struct Displays {
    pub gpus: Vec<Gpu>,
    pub monitors: Vec<Monitor>,
}

struct Found {
    rect: Rect,
    output: Option<IDXGIOutput>,
}

impl Displays {
    pub fn enumerate(notes: &mut Vec<String>) -> Result<Self> {
        let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory1() }?;
        let d2d: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?;
        let mut gpus = Vec::new();
        let mut monitors = Vec::new();
        let mut orphans = Vec::new();
        for adapter_index in 0.. {
            let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
                Ok(adapter) => adapter,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(error) => return Err(error),
            };
            let found = attached_outputs(&adapter, notes)?;
            if found.is_empty() {
                continue;
            }
            match Gpu::create(&factory, &adapter, &d2d) {
                Ok(gpu) => {
                    let gpu_index = gpus.len();
                    gpus.push(gpu);
                    monitors.extend(found.into_iter().map(|found| Monitor {
                        rect: found.rect,
                        gpu: gpu_index,
                        output: found.output,
                    }));
                }
                Err(error) => {
                    notes.push(format!(
                        "adapter {adapter_index} has no usable device: {error}"
                    ));
                    orphans.extend(found);
                }
            }
        }
        if !gpus.is_empty() {
            monitors.extend(orphans.into_iter().map(|found| Monitor {
                rect: found.rect,
                gpu: 0,
                output: None,
            }));
        }
        Ok(Self { gpus, monitors })
    }
}

fn attached_outputs(adapter: &IDXGIAdapter1, notes: &mut Vec<String>) -> Result<Vec<Found>> {
    let mut found = Vec::new();
    for output_index in 0.. {
        let output = match unsafe { adapter.EnumOutputs(output_index) } {
            Ok(output) => output,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(error),
        };
        let description = unsafe { output.GetDesc() }?;
        if !description.AttachedToDesktop.as_bool() {
            continue;
        }
        let bounds = description.DesktopCoordinates;
        let rect = Rect::new(bounds.left, bounds.top, bounds.right, bounds.bottom);
        let rotated = description.Rotation != DXGI_MODE_ROTATION_IDENTITY
            && description.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED;
        if rotated {
            notes.push(format!(
                "monitor {}x{} is rotated; it is captured with GDI",
                rect.width(),
                rect.height()
            ));
        }
        found.push(Found {
            rect,
            output: (!rotated).then_some(output),
        });
    }
    Ok(found)
}
