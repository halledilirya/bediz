//! What DirectComposition shows of a pin: its image, scaled and faded on the graphics card so a
//! zoom or a fade never draws its pixels again, and its border as four edges outside the image.

use bediz_core::image::Image;
use bediz_core::picture;
use bediz_core::pin::{ACTUAL, Look};
use windows::Win32::Foundation::{E_FAIL, E_UNEXPECTED, HWND, POINT};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectComposition::{
    DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR,
    DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, DCOMPOSITION_BORDER_MODE_HARD,
    DCOMPOSITION_OPACITY_MODE_MULTIPLY, IDCompositionSurface, IDCompositionTarget,
    IDCompositionVisual, IDCompositionVisual2, IDCompositionVisual3,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
};
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::core::{Error, Interface, Result};
use windows_numerics::Matrix3x2;

use super::Devices;

const EDGES: usize = 4;

pub struct Composed {
    _target: IDCompositionTarget,
    root: IDCompositionVisual2,
    image: IDCompositionVisual2,
    size: (u32, u32),
    edges: Vec<IDCompositionVisual2>,
    edge: IDCompositionSurface,
    _surface: IDCompositionSurface,
}

impl Composed {
    // A picture larger than a texture holds is shown halved; it is still copied and saved whole.
    pub fn create(devices: &Devices, window: HWND, image: &Image) -> Result<Self> {
        let composition = &devices.composition;
        let target = unsafe { composition.CreateTargetForHwnd(window, true) }?;
        let root = unsafe { composition.CreateVisual() }?;
        // The image and its edges never overlap, so each fades alone instead of through a layer.
        let _ = unsafe { root.SetOpacityMode(DCOMPOSITION_OPACITY_MODE_MULTIPLY) };
        let shown = picture::within(image, D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION)
            .ok_or_else(|| Error::new(E_FAIL, "the image could not be made smaller"))?;
        let size = (shown.width(), shown.height());
        let pixels = surface(devices, size.0, size.1, shown.bgra())?;
        let image_visual = unsafe { composition.CreateVisual() }?;
        unsafe {
            image_visual.SetContent(&pixels)?;
            image_visual.SetBorderMode(DCOMPOSITION_BORDER_MODE_HARD)?;
            root.AddVisual(&image_visual, true, None::<&IDCompositionVisual>)?;
        }
        let edge = surface(devices, 1, 1, &[0; 4])?;
        let mut edges = Vec::with_capacity(EDGES);
        for _ in 0..EDGES {
            let visual = unsafe { composition.CreateVisual() }?;
            unsafe {
                visual.SetContent(&edge)?;
                visual.SetBitmapInterpolationMode(
                    DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                )?;
                visual.SetBorderMode(DCOMPOSITION_BORDER_MODE_HARD)?;
                root.AddVisual(&visual, true, None::<&IDCompositionVisual>)?;
            }
            edges.push(visual);
        }
        unsafe { target.SetRoot(&root) }?;
        Ok(Self {
            _target: target,
            root,
            image: image_visual,
            size,
            edges,
            edge,
            _surface: pixels,
        })
    }

    // Enlarged pixels stay sharp squares to be read; a smaller pin is smoothed instead.
    pub fn lay_out(&self, look: Look, (width, height): (u32, u32), border: u32) -> Result<()> {
        let border = border as f32;
        let (width, height) = (width as f32, height as f32);
        let side = (height - 2.0 * border).max(0.0);
        // The image fills the window's whole pixels, which its zoomed size is rounded to.
        let across = (width - 2.0 * border).max(0.0) / self.size.0 as f32;
        let down = side / self.size.1 as f32;
        let interpolation = if look.zoom >= ACTUAL {
            DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR
        } else {
            DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR
        };
        unsafe {
            self.image
                .SetTransform2(&matrix(across, down, border, border))?;
            self.image.SetBitmapInterpolationMode(interpolation)?;
        }
        let edges = [
            matrix(width, border, 0.0, 0.0),
            matrix(width, border, 0.0, height - border),
            matrix(border, side, 0.0, border),
            matrix(border, side, width - border, border),
        ];
        for (visual, placed) in self.edges.iter().zip(edges) {
            unsafe { visual.SetTransform2(&placed) }?;
        }
        let root: IDCompositionVisual3 = self.root.cast()?;
        unsafe { root.SetOpacity2(look.alpha()) }
    }

    pub fn paint_border(&self, devices: &Devices, colour: [u8; 4]) -> Result<()> {
        fill(devices, &self.edge, 1, 1, &colour)
    }
}

fn matrix(across: f32, down: f32, x: f32, y: f32) -> Matrix3x2 {
    Matrix3x2 {
        M11: across,
        M12: 0.0,
        M21: 0.0,
        M22: down,
        M31: x,
        M32: y,
    }
}

fn surface(
    devices: &Devices,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> Result<IDCompositionSurface> {
    let surface = unsafe {
        devices.composition.CreateSurface(
            width,
            height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )
    }?;
    fill(devices, &surface, width, height, bgra)?;
    Ok(surface)
}

// A small surface may be a part of a larger texture, found at the offset the drawing begins at.
fn fill(
    devices: &Devices,
    surface: &IDCompositionSurface,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> Result<()> {
    let mut offset = POINT::default();
    let drawn: IDXGISurface = unsafe { surface.BeginDraw(None, &mut offset) }?;
    let written = copy_into(devices, &drawn, offset, width, height, bgra);
    let ended = unsafe { surface.EndDraw() };
    written.and(ended)
}

fn copy_into(
    devices: &Devices,
    drawn: &IDXGISurface,
    offset: POINT,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> Result<()> {
    let unexpected = |_| Error::from(E_UNEXPECTED);
    let texture: ID3D11Texture2D = drawn.cast()?;
    let left = u32::try_from(offset.x).map_err(unexpected)?;
    let top = u32::try_from(offset.y).map_err(unexpected)?;
    let needed = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if needed != Some(bgra.len()) {
        return Err(Error::from(E_UNEXPECTED));
    }
    let place = D3D11_BOX {
        left,
        top,
        front: 0,
        right: left.saturating_add(width),
        bottom: top.saturating_add(height),
        back: 1,
    };
    unsafe {
        devices.context.UpdateSubresource(
            &texture,
            0,
            Some(&place),
            bgra.as_ptr().cast(),
            width.saturating_mul(4),
            0,
        );
    }
    Ok(())
}
