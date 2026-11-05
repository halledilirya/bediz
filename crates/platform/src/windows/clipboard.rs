use bediz_core::image::Image;
use windows::Win32::Foundation::{E_OUTOFMEMORY, GlobalFree, HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{BI_BITFIELDS, BITMAPV5HEADER, LCS_GM_IMAGES};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIBV5, CF_UNICODETEXT};
use windows::Win32::UI::ColorSystem::LCS_sRGB;
use windows::core::{Error, Result, w};

pub enum Contents {
    Image { png: Vec<u8>, dib: Vec<u8> },
    Text(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    Done,
    Busy,
}

impl Contents {
    pub fn image(image: &Image, png: Vec<u8>) -> Self {
        let pixels = image.opaque_bottom_up_bgra();
        let header = dib_header(image, pixels.len());
        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                (&header as *const BITMAPV5HEADER).cast::<u8>(),
                size_of::<BITMAPV5HEADER>(),
            )
        };
        let mut dib = Vec::with_capacity(header_bytes.len() + pixels.len());
        dib.extend_from_slice(header_bytes);
        dib.extend_from_slice(&pixels);
        Self::Image { png, dib }
    }

    pub fn text(text: &str) -> Self {
        Self::Text(
            text.encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        )
    }
}

// The cloud flag goes first and a failure empties the clipboard, so content never stays without it.
pub fn place(owner: HWND, contents: &Contents) -> Result<Placed> {
    if unsafe { OpenClipboard(Some(owner)) }.is_err() {
        return Ok(Placed::Busy);
    }
    let result = unsafe { EmptyClipboard() }
        .and_then(|()| keep_off_cloud())
        .and_then(|()| match contents {
            Contents::Image { png, dib } => {
                set(unsafe { RegisterClipboardFormatW(w!("PNG")) }, png)?;
                set(u32::from(CF_DIBV5.0), dib)
            }
            Contents::Text(utf16) => set(u32::from(CF_UNICODETEXT.0), utf16),
        });
    if result.is_err() {
        let _ = unsafe { EmptyClipboard() };
    }
    let closed = unsafe { CloseClipboard() };
    result.and(closed).map(|()| Placed::Done)
}

fn keep_off_cloud() -> Result<()> {
    set(
        unsafe { RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")) },
        &0u32.to_le_bytes(),
    )
}

fn set(format: u32, bytes: &[u8]) -> Result<()> {
    if format == 0 {
        return Err(Error::from_thread());
    }
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }?;
    let target = unsafe { GlobalLock(memory) };
    if target.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(E_OUTOFMEMORY.into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(memory);
    }
    match unsafe { SetClipboardData(format, Some(HANDLE(memory.0))) } {
        Ok(_) => Ok(()),
        Err(error) => {
            let _ = unsafe { GlobalFree(Some(memory)) };
            Err(error)
        }
    }
}

fn dib_header(image: &Image, size: usize) -> BITMAPV5HEADER {
    BITMAPV5HEADER {
        bV5Size: size_of::<BITMAPV5HEADER>() as u32,
        bV5Width: i32::try_from(image.width()).unwrap_or(i32::MAX),
        bV5Height: i32::try_from(image.height()).unwrap_or(i32::MAX),
        bV5Planes: 1,
        bV5BitCount: 32,
        bV5Compression: BI_BITFIELDS,
        bV5SizeImage: u32::try_from(size).unwrap_or(u32::MAX),
        bV5RedMask: 0x00ff_0000,
        bV5GreenMask: 0x0000_ff00,
        bV5BlueMask: 0x0000_00ff,
        bV5AlphaMask: 0xff00_0000,
        bV5CSType: LCS_sRGB.0 as u32,
        bV5Intent: LCS_GM_IMAGES as u32,
        ..Default::default()
    }
}
