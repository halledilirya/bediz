//! A picture to mark up laid over an overlay's live screen: drawn once at the size it is shown,
//! with a checker under its see-through parts, then copied back over every new frame.

use bediz_core::geometry::Rect;
use bediz_core::image::Image;
use bediz_core::picture;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_BRUSH_PROPERTIES1, D2D1_EXTEND_MODE_WRAP,
    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    ID2D1Image,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::core::{Error, Result};

use crate::windows::gpu::Gpu;

const BLACK: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const CHECKER_SQUARE: f32 = 8.0;

pub struct Laid {
    texture: ID3D11Texture2D,
    left: u32,
    top: u32,
}

impl Laid {
    pub fn new(gpu: &Gpu, picture: &Image, place: Rect, scale: f32) -> Result<Self> {
        let (Ok(left), Ok(top)) = (u32::try_from(place.left), u32::try_from(place.top)) else {
            return Err(Error::new(E_FAIL, "the picture lies outside its monitor"));
        };
        let (width, height) = (place.width(), place.height());
        let texture = gpu.image_texture(width, height)?;
        let target = gpu.drawable_bitmap(&texture)?;
        unsafe {
            gpu.canvas.SetTarget(&target);
            gpu.canvas.BeginDraw();
            gpu.canvas.Clear(Some(&BLACK));
        }
        let drawn = draw(gpu, picture, width, height, scale);
        let ended = unsafe { gpu.canvas.EndDraw(None, None) };
        unsafe { gpu.canvas.SetTarget(None::<&ID2D1Image>) };
        drawn.and(ended)?;
        Ok(Self { texture, left, top })
    }

    pub fn copy_into(&self, gpu: &Gpu, image: &ID3D11Texture2D) {
        unsafe {
            gpu.context.CopySubresourceRegion(
                image,
                0,
                self.left,
                self.top,
                0,
                &self.texture,
                0,
                None,
            );
        }
    }
}

fn draw(gpu: &Gpu, picture: &Image, width: u32, height: u32, scale: f32) -> Result<()> {
    let bitmap = gpu.picture_bitmap(picture)?;
    let area = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: width as f32,
        bottom: height as f32,
    };
    if !picture.is_opaque() {
        let square = (CHECKER_SQUARE * scale).round() as u32;
        let checker = picture::checker(square)
            .ok_or_else(|| Error::new(E_FAIL, "the checker could not be made"))?;
        let pattern = gpu.memory_bitmap(&checker)?;
        let tiled = D2D1_BITMAP_BRUSH_PROPERTIES1 {
            extendModeX: D2D1_EXTEND_MODE_WRAP,
            extendModeY: D2D1_EXTEND_MODE_WRAP,
            interpolationMode: D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        };
        let brush = unsafe { gpu.canvas.CreateBitmapBrush(&pattern, Some(&tiled), None) }?;
        unsafe { gpu.canvas.FillRectangle(&area, &brush) };
    }
    let whole = width == picture.width() && height == picture.height();
    let interpolation = if whole {
        D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR
    } else {
        D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC
    };
    unsafe {
        gpu.canvas
            .DrawBitmap(&bitmap, Some(&area), 1.0, interpolation, None, None);
    }
    Ok(())
}
