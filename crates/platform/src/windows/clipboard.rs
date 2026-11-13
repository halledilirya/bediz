//! Puts images, text and files on the clipboard, always kept from the cloud clipboard, with images
//! and files also kept out of the clipboard history when asked.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;

use bediz_core::image::Image;
use windows::Win32::Foundation::{E_OUTOFMEMORY, GlobalFree, HANDLE, HWND, POINT};
use windows::Win32::Graphics::Gdi::{BI_BITFIELDS, BITMAPV5HEADER, LCS_GM_IMAGES};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIBV5, CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY};
use windows::Win32::UI::ColorSystem::LCS_sRGB;
use windows::Win32::UI::Shell::DROPFILES;
use windows::core::{Error, PCWSTR, Result, w};

pub enum Contents {
    Image { png: Arc<Vec<u8>>, dib: Vec<u8> },
    Text(Vec<u8>),
    File(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    Done,
    Busy,
    Changed,
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

    pub fn file(path: &Path) -> Self {
        let header = DROPFILES {
            pFiles: size_of::<DROPFILES>() as u32,
            pt: POINT::default(),
            fNC: false.into(),
            fWide: true.into(),
        };
        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                (&header as *const DROPFILES).cast::<u8>(),
                size_of::<DROPFILES>(),
            )
        };
        let mut list = header_bytes.to_vec();
        // Each name ends in a null, and the list in one more.
        for unit in path.as_os_str().encode_wide().chain([0, 0]) {
            list.extend_from_slice(&unit.to_le_bytes());
        }
        Self::File(list)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    NoCloud,
    NoHistory,
    Png,
    Bitmap,
    Text,
    Files,
    DropEffect,
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
            Self::Files => u32::from(CF_HDROP.0),
            Self::DropEffect => registered(w!("Preferred DropEffect")),
        }
    }
}

trait Board {
    fn changes(&self) -> u32;
    fn empty(&mut self) -> Result<()>;
    fn put(&mut self, format: Format, bytes: &[u8]) -> Result<()>;
}

struct Clipboard;

impl Board for Clipboard {
    fn changes(&self) -> u32 {
        changes()
    }

    fn empty(&mut self) -> Result<()> {
        unsafe { EmptyClipboard() }
    }

    fn put(&mut self, format: Format, bytes: &[u8]) -> Result<()> {
        set(format.id(), bytes)
    }
}

const REFUSED: [u8; 4] = 0u32.to_le_bytes();

pub fn changes() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub fn place(
    owner: HWND,
    contents: &Contents,
    keep_out_of_history: bool,
    unchanged_since: Option<u32>,
) -> Result<Placed> {
    if unsafe { OpenClipboard(Some(owner)) }.is_err() {
        return Ok(Placed::Busy);
    }
    let filled = fill(
        &mut Clipboard,
        contents,
        keep_out_of_history,
        unchanged_since,
    );
    let closed = unsafe { CloseClipboard() };
    filled.and_then(|placed| closed.map(|()| placed))
}

// The flags go first and a failure empties the clipboard, so content never stays without them.
fn fill(
    board: &mut impl Board,
    contents: &Contents,
    keep_out_of_history: bool,
    unchanged_since: Option<u32>,
) -> Result<Placed> {
    // Read while the clipboard is open, so no other app can change it before it is emptied.
    if unchanged_since.is_some_and(|count| board.changes() != count) {
        return Ok(Placed::Changed);
    }
    let filled = put_all(board, contents, keep_out_of_history);
    if filled.is_err() {
        let _ = board.empty();
    }
    filled.map(|()| Placed::Done)
}

fn put_all(board: &mut impl Board, contents: &Contents, keep_out_of_history: bool) -> Result<()> {
    board.empty()?;
    board.put(Format::NoCloud, &REFUSED)?;
    if keep_out_of_history && !matches!(contents, Contents::Text(_)) {
        board.put(Format::NoHistory, &REFUSED)?;
    }
    match contents {
        Contents::Image { png, dib } => {
            board.put(Format::Png, png)?;
            board.put(Format::Bitmap, dib)
        }
        Contents::Text(utf16) => board.put(Format::Text, utf16),
        Contents::File(list) => {
            board.put(Format::Files, list)?;
            board.put(Format::DropEffect, &DROPEFFECT_COPY.0.to_le_bytes())
        }
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

    use super::{Board, Contents, Format, Placed, REFUSED, fill};

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Empty,
        Put(Format, usize),
    }

    struct Recorded {
        calls: Vec<Call>,
        failing: Option<Format>,
        changes: u32,
    }

    impl Board for Recorded {
        fn changes(&self) -> u32 {
            self.changes
        }

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
            changes: 0,
        };
        let filled = fill(&mut board, contents, keep_out_of_history, None);
        assert_eq!(filled.is_err(), failing.is_some());
        board.calls
    }

    fn placed_unless_changed(changes: u32, unchanged_since: u32) -> (Placed, Vec<Call>) {
        let mut board = Recorded {
            calls: Vec::new(),
            failing: None,
            changes,
        };
        let file = Contents::file(std::path::Path::new(r"C:\GIFs\a.gif"));
        let filled = fill(&mut board, &file, false, Some(unchanged_since)).unwrap();
        (filled, board.calls)
    }

    #[test]
    fn a_gif_whose_clipboard_changed_since_its_stop_leaves_the_clipboard_alone() {
        assert_eq!(placed_unless_changed(8, 7), (Placed::Changed, Vec::new()));
    }

    #[test]
    fn a_gif_whose_clipboard_is_as_it_was_at_its_stop_goes_on() {
        let (filled, calls) = placed_unless_changed(7, 7);
        assert_eq!(filled, Placed::Done);
        assert_eq!(calls.first(), Some(&Call::Empty));
        assert_eq!(calls.len(), 4);
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
    fn a_gif_goes_on_as_a_file_to_copy_and_stays_out_of_history_when_asked() {
        let file = Contents::file(std::path::Path::new(r"C:\GIFs\a.gif"));
        let Contents::File(list) = &file else {
            panic!("a file is not on the clipboard as a file");
        };
        assert_eq!(
            placed(&file, true, None),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Put(Format::NoHistory, REFUSED.len()),
                Call::Put(Format::Files, list.len()),
                Call::Put(Format::DropEffect, 4),
            ]
        );
        assert_eq!(
            placed(&file, false, None),
            [
                Call::Empty,
                Call::Put(Format::NoCloud, REFUSED.len()),
                Call::Put(Format::Files, list.len()),
                Call::Put(Format::DropEffect, 4),
            ]
        );
    }

    #[test]
    fn the_file_list_holds_its_header_the_wide_name_and_two_nulls() {
        let Contents::File(list) = Contents::file(std::path::Path::new(r"C:\ğ.gif")) else {
            panic!("a file is not on the clipboard as a file");
        };
        let header = size_of::<windows::Win32::UI::Shell::DROPFILES>();
        assert_eq!(header, 20);
        assert_eq!(list.get(..4), Some(&20u32.to_le_bytes()[..]));
        assert_eq!(list.get(16..20), Some(&1i32.to_le_bytes()[..]));
        let name: Vec<u16> = list
            .get(header..)
            .unwrap()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let mut expected: Vec<u16> = r"C:\ğ.gif".encode_utf16().collect();
        expected.extend([0, 0]);
        assert_eq!(name, expected);
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
