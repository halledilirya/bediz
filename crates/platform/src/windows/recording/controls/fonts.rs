//! The fonts of the controls bar: the system's message font, the icon font when it is installed,
//! and the width of texts set in them.

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CreateFontIndirectW, DEFAULT_CHARSET, DeleteObject, FW_NORMAL, GetDC,
    GetTextExtentPoint32W, GetTextFaceW, HDC, HFONT, LOGFONTW, ReleaseDC, SelectObject,
};
use windows::Win32::UI::HiDpi::SystemParametersInfoForDpi;
use windows::Win32::UI::WindowsAndMessaging::{NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS};
use windows::core::{Error, Result};

use super::{pixels, scale};

const ICON_SIZE: f32 = 16.0;
const ICON_FACE: &str = "Segoe Fluent Icons";

pub(super) struct Font(pub(super) HFONT);

impl Drop for Font {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.0.into()) };
    }
}

pub(super) fn message_font(dpi: u32) -> Result<Font> {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    unsafe {
        SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()),
            0,
            dpi,
        )
    }?;
    let font = unsafe { CreateFontIndirectW(&metrics.lfMessageFont) };
    if font.is_invalid() {
        return Err(Error::from_thread());
    }
    Ok(Font(font))
}

// GDI quietly puts another font in place of a missing one, so the face actually chosen is checked.
pub(super) fn icon_font(dpi: u32) -> Option<Font> {
    let mut face = [0u16; 32];
    for (unit, wanted) in face.iter_mut().zip(ICON_FACE.encode_utf16()) {
        *unit = wanted;
    }
    let logical = LOGFONTW {
        lfHeight: -pixels(ICON_SIZE, scale(dpi)),
        lfWeight: FW_NORMAL.0 as i32,
        lfCharSet: DEFAULT_CHARSET,
        lfQuality: CLEARTYPE_QUALITY,
        lfFaceName: face,
        ..Default::default()
    };
    let font = unsafe { CreateFontIndirectW(&logical) };
    if font.is_invalid() {
        return None;
    }
    let font = Font(font);
    let dc = unsafe { GetDC(None) };
    let previous = unsafe { SelectObject(dc, font.0.into()) };
    let found = is_icon_face(dc);
    unsafe {
        SelectObject(dc, previous);
        ReleaseDC(None, dc);
    }
    found.then_some(font)
}

pub(super) fn is_icon_face(dc: HDC) -> bool {
    let mut name = [0u16; 32];
    let copied = usize::try_from(unsafe { GetTextFaceW(dc, Some(&mut name)) }).unwrap_or(0);
    let name = name.get(..copied).unwrap_or_default();
    let length = name
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(name.len());
    name.get(..length)
        .is_some_and(|name| name.iter().copied().eq(ICON_FACE.encode_utf16()))
}

pub(super) fn measure(font: &Font, texts: &[&str]) -> Vec<i32> {
    let dc = unsafe { GetDC(None) };
    let previous = unsafe { SelectObject(dc, font.0.into()) };
    let widths = texts
        .iter()
        .map(|text| {
            let wide: Vec<u16> = text.encode_utf16().collect();
            let mut size = SIZE::default();
            let _ = unsafe { GetTextExtentPoint32W(dc, &wide, &mut size) };
            size.cx
        })
        .collect();
    unsafe {
        SelectObject(dc, previous);
        ReleaseDC(None, dc);
    }
    widths
}
