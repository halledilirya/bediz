use bediz_core::image::Image;
use windows::Win32::Foundation::{E_OUTOFMEMORY, GlobalFree, HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{BI_BITFIELDS, BITMAPV5HEADER, LCS_GM_IMAGES};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIBV5, CF_UNICODETEXT};
use windows::Win32::System::Threading::Sleep;
use windows::Win32::UI::ColorSystem::LCS_sRGB;
use windows::core::{Error, Result, w};

const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY_MILLISECONDS: u32 = 25;

pub fn copy_image(owner: HWND, image: &Image, png: &[u8]) -> Result<()> {
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
    publish(owner, || {
        set(unsafe { RegisterClipboardFormatW(w!("PNG")) }, png)?;
        set(u32::from(CF_DIBV5.0), &dib)
    })
}

pub fn copy_text(owner: HWND, text: &str) -> Result<()> {
    let utf16: Vec<u8> = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    publish(owner, || set(u32::from(CF_UNICODETEXT.0), &utf16))
}

// The cloud flag goes first and a failure empties the clipboard, so content never stays without it.
fn publish(owner: HWND, formats: impl FnOnce() -> Result<()>) -> Result<()> {
    open(owner)?;
    let result = unsafe { EmptyClipboard() }
        .and_then(|()| keep_off_cloud())
        .and_then(|()| formats());
    if result.is_err() {
        let _ = unsafe { EmptyClipboard() };
    }
    let closed = unsafe { CloseClipboard() };
    result.and(closed)
}

fn open(owner: HWND) -> Result<()> {
    let mut last = Ok(());
    for attempt in 0..OPEN_ATTEMPTS {
        if attempt > 0 {
            unsafe { Sleep(OPEN_RETRY_MILLISECONDS) };
        }
        last = unsafe { OpenClipboard(Some(owner)) };
        if last.is_ok() {
            return last;
        }
    }
    last
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
