use std::sync::Arc;

use bediz_core::image::Image;
use windows::Win32::Foundation::{E_OUTOFMEMORY, GlobalFree, HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{BI_BITFIELDS, BITMAPV5HEADER, LCS_GM_IMAGES};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIBV5, CF_UNICODETEXT};
use windows::Win32::UI::ColorSystem::LCS_sRGB;
use windows::core::{Error, PCWSTR, Result, w};

pub enum Contents {
    Image { png: Arc<Vec<u8>>, dib: Vec<u8> },
    Text(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    Done,
    Busy,
}

impl Contents {
    pub fn image(image: &Image, png: Arc<Vec<u8>>) -> Self {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    NoCloud,
    NoHistory,
    Png,
    Bitmap,
    Text,
}

impl Format {
    fn id(self) -> u32 {
        let registered = |name: PCWSTR| unsafe { RegisterClipboardFormatW(name) };
        match self {
            Self::NoCloud => registered(w!("CanUploadToCloudClipboard")),
            Self::NoHistory => registered(w!("CanIncludeInClipboardHistory")),
            Self::Png => registered(w!("PNG")),
            Self::Bitmap => u32::from(CF_DIBV5.0),
            Self::Text => u32::from(CF_UNICODETEXT.0),
        }
    }
}

trait Board {
    fn empty(&mut self) -> Result<()>;
    fn put(&mut self, format: Format, bytes: &[u8]) -> Result<()>;
}

struct Clipboard;

impl Board for Clipboard {
    fn empty(&mut self) -> Result<()> {
        unsafe { EmptyClipboard() }
    }

    fn put(&mut self, format: Format, bytes: &[u8]) -> Result<()> {
        set(format.id(), bytes)
    }
}

const REFUSED: [u8; 4] = 0u32.to_le_bytes();

pub fn place(owner: HWND, contents: &Contents, keep_out_of_history: bool) -> Result<Placed> {
    if unsafe { OpenClipboard(Some(owner)) }.is_err() {
        return Ok(Placed::Busy);
    }
    let filled = fill(&mut Clipboard, contents, keep_out_of_history);
    let closed = unsafe { CloseClipboard() };
    filled.and(closed).map(|()| Placed::Done)
}

// The flags go first and a failure empties the clipboard, so content never stays without them.
fn fill(board: &mut impl Board, contents: &Contents, keep_out_of_history: bool) -> Result<()> {
    let filled = put_all(board, contents, keep_out_of_history);
    if filled.is_err() {
        let _ = board.empty();
    }
    filled
}

fn put_all(board: &mut impl Board, contents: &Contents, keep_out_of_history: bool) -> Result<()> {
    board.empty()?;
    board.put(Format::NoCloud, &REFUSED)?;
    if keep_out_of_history && matches!(contents, Contents::Image { .. }) {
        board.put(Format::NoHistory, &REFUSED)?;
    }
    match contents {
        Contents::Image { png, dib } => {
            board.put(Format::Png, png)?;
            board.put(Format::Bitmap, dib)
        }
        Contents::Text(utf16) => board.put(Format::Text, utf16),
    }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use windows::Win32::Foundation::E_FAIL;
    use windows::core::Result;

    use super::{Board, Contents, Format, REFUSED, fill};

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Empty,
        Put(Format, usize),
    }

    struct Recorded {
        calls: Vec<Call>,
        failing: Option<Format>,
    }

    impl Board for Recorded {
        fn empty(&mut self) -> Result<()> {
            self.calls.push(Call::Empty);
            Ok(())
        }

        fn put(&mut self, format: Format, bytes: &[u8]) -> Result<()> {
            self.calls.push(Call::Put(format, bytes.len()));
            if self.failing == Some(format) {
                return Err(E_FAIL.into());
            }
            Ok(())
        }
    }

    fn placed(
        contents: &Contents,
        keep_out_of_history: bool,
        failing: Option<Format>,
    ) -> Vec<Call> {
        let mut board = Recorded {
            calls: Vec::new(),
            failing,
        };
        let filled = fill(&mut board, contents, keep_out_of_history);
        assert_eq!(filled.is_err(), failing.is_some());
        board.calls
    }

    fn image() -> Contents {
        Contents::Image {
            png: Arc::new(vec![1; 10]),
            dib: vec![2; 20],
        }
    }

    #[test]
    fn the_flags_go_on_before_any_screenshot_content() {
        assert_eq!(
            placed(&image(), true, None),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Put(Format::NoHistory, REFUSED.len()),
                Call::Put(Format::Png, 10),
                Call::Put(Format::Bitmap, 20),
            ]
        );
        assert_eq!(
            placed(&image(), false, None),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Put(Format::Png, 10),
                Call::Put(Format::Bitmap, 20),
            ]
        );
    }

    #[test]
    fn a_copied_colour_stays_out_of_the_cloud_but_in_the_history() {
        assert_eq!(
            placed(&Contents::text("#1A2B3C"), true, None),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Put(Format::Text, 16),
            ]
        );
    }

    #[test]
    fn a_flag_that_cannot_be_set_stops_the_content_and_empties_the_clipboard() {
        assert_eq!(
            placed(&image(), true, Some(Format::NoCloud)),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Empty,
            ]
        );
        assert_eq!(
            placed(&image(), true, Some(Format::NoHistory)),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Put(Format::NoHistory, REFUSED.len()),
                Call::Empty,
            ]
        );
    }

    #[test]
    fn content_that_fails_halfway_is_taken_off_again() {
        assert_eq!(
            placed(&image(), false, Some(Format::Bitmap)),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Put(Format::Png, 10),
                Call::Put(Format::Bitmap, 20),
                Call::Empty,
            ]
        );
    }
}
