use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, ID3D11Texture2D,
};
use windows::core::{Error, Result};

use super::gpu::Gpu;

pub fn read(gpu: &Gpu, source: &ID3D11Texture2D, area: Rect) -> Result<Image> {
    let (Ok(left), Ok(top), Ok(right), Ok(bottom)) = (
        u32::try_from(area.left),
        u32::try_from(area.top),
        u32::try_from(area.right),
        u32::try_from(area.bottom),
    ) else {
        return Err(E_INVALIDARG.into());
    };
    let (width, height) = (area.width(), area.height());
    let staging = gpu.staging_texture(width, height)?;
    let region = D3D11_BOX {
        left,
        top,
        front: 0,
        right,
        bottom,
        back: 1,
    };
    unsafe {
        gpu.context
            .CopySubresourceRegion(&staging, 0, 0, 0, 0, source, 0, Some(&region));
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        gpu.context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
    }?;
    let row_bytes = width as usize * 4;
    let mut pixels = Vec::with_capacity(row_bytes * height as usize);
    let base = mapped.pData.cast::<u8>().cast_const();
    for row in 0..height as usize {
        let start = unsafe { base.add(row * mapped.RowPitch as usize) };
        pixels.extend_from_slice(unsafe { std::slice::from_raw_parts(start, row_bytes) });
    }
    unsafe { gpu.context.Unmap(&staging, 0) };
    Image::from_bgra(width, height, pixels)
        .map_err(|error| Error::new(E_INVALIDARG, error.to_string()))
}

pub fn read_pixel(gpu: &Gpu, source: &ID3D11Texture2D, at: Point) -> Result<[u8; 4]> {
    let (Ok(x), Ok(y)) = (u32::try_from(at.x), u32::try_from(at.y)) else {
        return Err(E_INVALIDARG.into());
    };
    let region = D3D11_BOX {
        left: x,
        top: y,
        front: 0,
        right: x.saturating_add(1),
        bottom: y.saturating_add(1),
        back: 1,
    };
    unsafe {
        gpu.context
            .CopySubresourceRegion(&gpu.pixel, 0, 0, 0, 0, source, 0, Some(&region));
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        gpu.context
            .Map(&gpu.pixel, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
    }?;
    let mut bgra = [0u8; 4];
    unsafe {
        std::ptr::copy_nonoverlapping(mapped.pData.cast::<u8>(), bgra.as_mut_ptr(), 4);
        gpu.context.Unmap(&gpu.pixel, 0);
    }
    Ok(bgra)
}
