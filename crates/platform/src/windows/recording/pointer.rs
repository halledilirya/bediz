use std::rc::Rc;

use bediz_core::cursor::Cursor;
use bediz_core::geometry::Point;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, ReleaseDC,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, GetCursorInfo, GetIconInfo, HCURSOR, HICON, ICONINFO,
};

#[derive(Clone)]
pub struct Sample {
    pub at: Point,
    pub cursor: Rc<Cursor>,
}

#[derive(Default)]
pub struct Pointer {
    shape: Option<(HCURSOR, Option<Rc<Cursor>>)>,
}

struct Bitmaps(HBITMAP, HBITMAP);

impl Drop for Bitmaps {
    fn drop(&mut self) {
        for bitmap in [self.0, self.1] {
            if !bitmap.is_invalid() {
                let _ = unsafe { DeleteObject(bitmap.into()) };
            }
        }
    }
}

impl Sample {
    pub fn same_as(&self, other: &Self) -> bool {
        self.at == other.at && Rc::ptr_eq(&self.cursor, &other.cursor)
    }
}

impl Pointer {
    pub fn sample(&mut self) -> Option<Sample> {
        let mut info = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetCursorInfo(&mut info) }.ok()?;
        if info.flags.0 & CURSOR_SHOWING.0 == 0 || info.hCursor.is_invalid() {
            return None;
        }
        let cursor = match &self.shape {
            Some((handle, known)) if *handle == info.hCursor => known.clone(),
            _ => {
                let made = shape(info.hCursor).map(Rc::new);
                self.shape = Some((info.hCursor, made.clone()));
                made
            }
        }?;
        Some(Sample {
            at: Point::new(info.ptScreenPos.x, info.ptScreenPos.y),
            cursor,
        })
    }
}

fn shape(handle: HCURSOR) -> Option<Cursor> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(HICON(handle.0), &mut info) }.ok()?;
    let bitmaps = Bitmaps(info.hbmMask, info.hbmColor);
    let hotspot = Point::new(
        i32::try_from(info.xHotspot).ok()?,
        i32::try_from(info.yHotspot).ok()?,
    );
    let (width, mask_height, mask) = pixels(bitmaps.0)?;
    let mask: Vec<bool> = mask.as_chunks::<4>().0.iter().map(is_set).collect();
    if bitmaps.1.is_invalid() {
        let height = mask_height / 2;
        let (and, xor) = mask.split_at_checked(width as usize * height as usize)?;
        return Cursor::from_monochrome(width, height, hotspot, and, xor);
    }
    let (colour_width, height, bgra) = pixels(bitmaps.1)?;
    if colour_width != width || height != mask_height {
        return None;
    }
    Cursor::from_colour(width, height, hotspot, &bgra, &mask)
}

fn is_set(pixel: &[u8; 4]) -> bool {
    let [blue, green, red, _] = *pixel;
    blue != 0 || green != 0 || red != 0
}

fn pixels(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut description = BITMAP::default();
    let read = unsafe {
        GetObjectW(
            bitmap.into(),
            size_of::<BITMAP>() as i32,
            Some((&mut description as *mut BITMAP).cast()),
        )
    };
    if read == 0 {
        return None;
    }
    let width = u32::try_from(description.bmWidth)
        .ok()
        .filter(|&side| side > 0)?;
    let height = description.bmHeight.unsigned_abs();
    let top_down = i32::try_from(height).ok().filter(|&side| side > 0)?;
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: description.bmWidth,
            biHeight: -top_down,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![
        0u8;
        (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?
    ];
    let screen = unsafe { GetDC(None) };
    let lines = unsafe {
        GetDIBits(
            screen,
            bitmap,
            0,
            height,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe { ReleaseDC(None, screen) };
    (u32::try_from(lines).ok() == Some(height)).then_some((width, height, bgra))
}
