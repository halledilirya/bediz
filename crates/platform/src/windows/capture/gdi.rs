use std::ffi::c_void;

use bediz_core::geometry::Rect;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11Texture2D};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::core::{Error, Result};

pub struct GdiSurface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut c_void,
    width: u32,
    height: u32,
}

impl GdiSurface {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let (Ok(signed_width), Ok(signed_height)) = (i32::try_from(width), i32::try_from(height))
        else {
            return Err(Error::new(
                E_FAIL,
                "the monitor is too large for a GDI bitmap",
            ));
        };
        let screen = unsafe { GetDC(None) };
        let dc = unsafe { CreateCompatibleDC(Some(screen)) };
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: signed_width,
                biHeight: -signed_height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0) };
        unsafe { ReleaseDC(None, screen) };
        let bitmap = match bitmap {
            Ok(bitmap) if !dc.is_invalid() && !bits.is_null() => bitmap,
            Ok(bitmap) => {
                unsafe {
                    let _ = DeleteObject(bitmap.into());
                    let _ = DeleteDC(dc);
                }
                return Err(Error::new(E_FAIL, "the GDI bitmap could not be created"));
            }
            Err(error) => {
                let _ = unsafe { DeleteDC(dc) };
                return Err(error);
            }
        };
        let previous = unsafe { SelectObject(dc, bitmap.into()) };
        Ok(Self {
            dc,
            bitmap,
            previous,
            bits,
            width,
            height,
        })
    }

    pub fn capture(
        &self,
        rect: Rect,
        context: &ID3D11DeviceContext,
        target: &ID3D11Texture2D,
    ) -> Result<()> {
        let width = i32::try_from(self.width).unwrap_or(i32::MAX);
        let height = i32::try_from(self.height).unwrap_or(i32::MAX);
        let screen = unsafe { GetDC(None) };
        let copied = unsafe {
            BitBlt(
                self.dc,
                0,
                0,
                width,
                height,
                Some(screen),
                rect.left,
                rect.top,
                SRCCOPY,
            )
        };
        unsafe { ReleaseDC(None, screen) };
        copied?;
        let _ = unsafe { GdiFlush() };
        unsafe {
            context.UpdateSubresource(target, 0, None, self.bits.cast_const(), self.width * 4, 0);
        }
        Ok(())
    }
}

impl Drop for GdiSurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
