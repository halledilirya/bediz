//! Opens an image to mark up, from a file the user picks or from the clipboard, and reads it with
//! the Windows Imaging Component, upright and premultiplied, on a thread of its own.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bediz_core::image::Image;
use bediz_core::picture::{self, OpenError};
use windows::Win32::Foundation::{ERROR_CANCELLED, GENERIC_READ, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatBmp, GUID_WICPixelFormat32bppBGRA,
    IWICBitmapDecoder, IWICBitmapFrameDecode, IWICImagingFactory, IWICPalette,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PropVariantClear, PropVariantToUInt16,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{Error, HSTRING, PCWSTR, w};

use super::clipboard::{self, Kind, Pasted};
use super::runtime::WM_PICTURE;
use super::save;
use super::save_dialog::{file_path, shell_item};

const IMAGE_FILES: PCWSTR = w!(
    "*.png;*.jpg;*.jpeg;*.jpe;*.jfif;*.gif;*.bmp;*.dib;*.tif;*.tiff;*.webp;*.heic;*.heif;*.avif;*.jxr;*.wdp;*.ico"
);
// The Windows Imaging Component's own codes say the bytes are no image it can read.
const IMAGING_FACILITY: u32 = 0x8898;
const CLIPBOARD_ATTEMPTS: u32 = 10;
const CLIPBOARD_RETRY: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Clipboard,
    File {
        kind: String,
        usual_folder: Option<PathBuf>,
    },
}

pub struct Opened {
    pub request: u64,
    pub image: Option<Result<Image, OpenError>>,
}

pub type Pictures = Arc<Mutex<Option<Opened>>>;

// The common dialogs need a single-threaded apartment; the platform thread is multithreaded.
pub fn open(
    window: HWND,
    pictures: &Pictures,
    request: u64,
    source: Source,
) -> std::io::Result<()> {
    let pictures = Arc::clone(pictures);
    let window = window.0 as isize;
    std::thread::Builder::new()
        .name("bediz-picture".to_owned())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            let image = read(&source);
            if com {
                unsafe { CoUninitialize() };
            }
            *pictures.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(Opened { request, image });
            let window = HWND(window as *mut c_void);
            let _ = unsafe { PostMessageW(Some(window), WM_PICTURE, WPARAM(0), LPARAM(0)) };
        })?;
    Ok(())
}

fn read(source: &Source) -> Option<Result<Image, OpenError>> {
    match source {
        Source::Clipboard => Some(pasted()),
        Source::File { kind, usual_folder } => match pick(kind, usual_folder.as_deref()) {
            Ok(Some(path)) => Some(from_file(&path)),
            Ok(None) => None,
            Err(error) => Some(Err(OpenError::Unexpected(format!(
                "the file picker failed: {error}"
            )))),
        },
    }
}

fn pasted() -> Result<Image, OpenError> {
    let mut pasted = clipboard::image(copy_of);
    for _ in 1..CLIPBOARD_ATTEMPTS {
        if !matches!(pasted, Ok(Pasted::Busy)) {
            break;
        }
        std::thread::sleep(CLIPBOARD_RETRY);
        pasted = clipboard::image(copy_of);
    }
    match pasted.map_err(|error| OpenError::Other(error.message()))? {
        Pasted::Nothing => Err(OpenError::NoImage),
        Pasted::Busy => Err(OpenError::ClipboardBusy),
        Pasted::Image(copy) => from_bytes(&copy?),
        Pasted::File(path) => from_file(&path),
    }
}

// Checked while the clipboard still holds the bytes, so an image that is refused is never copied.
fn copy_of(kind: Kind, bytes: &[u8]) -> Result<Vec<u8>, OpenError> {
    let broken = || OpenError::Unreadable("the clipboard's bitmap has a broken header".to_owned());
    let (size, header) = match kind {
        Kind::Png => (picture::png_size(bytes), None),
        Kind::Bitmap => (
            Some(picture::bitmap_size(bytes).ok_or_else(broken)?),
            Some(picture::bitmap_header(bytes).ok_or_else(broken)?),
        ),
    };
    if let Some((width, height)) = size {
        picture::check_size(width, height)?;
    }
    picture::check_source(bytes.len())?;
    let header: &[u8] = match &header {
        Some(header) => header,
        None => &[],
    };
    let mut copy = Vec::new();
    copy.try_reserve_exact(header.len() + bytes.len())
        .map_err(|_| match size {
            Some((width, height)) => OpenError::TooLarge { width, height },
            None => OpenError::Unexpected(format!(
                "no memory was left to copy the clipboard's {} bytes",
                bytes.len()
            )),
        })?;
    copy.extend_from_slice(header);
    copy.extend_from_slice(bytes);
    Ok(copy)
}

fn pick(kind: &str, usual_folder: Option<&Path>) -> windows::core::Result<Option<PathBuf>> {
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }?;
    let name = HSTRING::from(kind);
    let types = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(name.as_ptr()),
        pszSpec: IMAGE_FILES,
    }];
    unsafe {
        dialog.SetFileTypes(&types)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST)?;
    }
    if let Some(item) = save::folder(usual_folder)
        .ok()
        .and_then(|folder| shell_item(&folder))
    {
        unsafe { dialog.SetDefaultFolder(&item) }?;
    }
    match unsafe { dialog.Show(None) } {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
        Err(error) => return Err(error),
    }
    file_path(&unsafe { dialog.GetResult() }?).map(Some)
}

fn from_file(path: &Path) -> Result<Image, OpenError> {
    let factory = factory()?;
    let decoder = unsafe {
        factory.CreateDecoderFromFilename(
            &HSTRING::from(path.as_os_str()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
    }
    .map_err(open_error)?;
    decode(&factory, &decoder)
}

fn from_bytes(bytes: &[u8]) -> Result<Image, OpenError> {
    if u32::try_from(bytes.len()).is_err() {
        return Err(OpenError::Unreadable(format!(
            "{} bytes are more than an image can be read from",
            bytes.len()
        )));
    }
    let factory = factory()?;
    let stream = unsafe { factory.CreateStream() }.map_err(unexpected)?;
    unsafe { stream.InitializeFromMemory(bytes) }.map_err(unexpected)?;
    let decoder = unsafe {
        factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
    }
    .map_err(open_error)?;
    decode(&factory, &decoder)
}

fn decode(factory: &IWICImagingFactory, decoder: &IWICBitmapDecoder) -> Result<Image, OpenError> {
    let frame = unsafe { decoder.GetFrame(0) }.map_err(open_error)?;
    let converter = unsafe { factory.CreateFormatConverter() }.map_err(unexpected)?;
    unsafe {
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .map_err(open_error)?;
    let (mut width, mut height) = (0, 0);
    unsafe { converter.GetSize(&mut width, &mut height) }.map_err(open_error)?;
    picture::check_size(width, height)?;
    let too_large = || OpenError::TooLarge { width, height };
    let stride = width.checked_mul(4).ok_or_else(too_large)?;
    let length = (stride as usize)
        .checked_mul(height as usize)
        .ok_or_else(too_large)?;
    let mut bgra = Vec::new();
    bgra.try_reserve_exact(length).map_err(|_| too_large())?;
    bgra.resize(length, 0);
    unsafe { converter.CopyPixels(std::ptr::null(), stride, &mut bgra) }.map_err(open_error)?;
    if unsafe { decoder.GetContainerFormat() }.is_ok_and(|format| format == GUID_ContainerFormatBmp)
    {
        picture::fill_unused_alpha(&mut bgra);
    }
    let image = picture::premultiplied(width, height, bgra)
        .map_err(|error| OpenError::Unexpected(error.to_string()))?;
    Ok(picture::upright(&image, orientation(&frame)).unwrap_or(image))
}

fn orientation(frame: &IWICBitmapFrameDecode) -> u16 {
    let Ok(reader) = (unsafe { frame.GetMetadataQueryReader() }) else {
        return 1;
    };
    let mut value = PROPVARIANT::default();
    let read = unsafe { reader.GetMetadataByName(w!("System.Photo.Orientation"), &mut value) }
        .and_then(|()| unsafe { PropVariantToUInt16(&value) });
    let _ = unsafe { PropVariantClear(&mut value) };
    read.unwrap_or(1)
}

fn factory() -> Result<IWICImagingFactory, OpenError> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .map_err(unexpected)
}

fn open_error(error: Error) -> OpenError {
    if (error.code().0 as u32) >> 16 == IMAGING_FACILITY {
        OpenError::Unreadable(error.message())
    } else {
        OpenError::Other(error.message())
    }
}

fn unexpected(error: Error) -> OpenError {
    OpenError::Unexpected(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use windows::Win32::Foundation::E_ACCESSDENIED;
    use windows::Win32::Graphics::Gdi::BITMAPV5HEADER;
    use windows::Win32::System::Com::COINIT_MULTITHREADED;
    use windows::core::HRESULT;

    use super::*;
    use crate::windows::clipboard::Contents;

    fn with_com(test: impl FnOnce()) {
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        test();
        if com {
            unsafe { CoUninitialize() };
        }
    }

    fn gradient(alpha: u8) -> Image {
        let bgra = (0..12u8)
            .flat_map(|index| [index * 20, 255 - index * 20, 128, alpha])
            .collect();
        Image::from_bgra(4, 3, bgra).unwrap()
    }

    #[test]
    fn a_png_reads_back_as_the_image_it_was_made_from() {
        with_com(|| {
            let image = gradient(u8::MAX);
            let read = from_bytes(&image.encode_png().unwrap()).unwrap();
            assert_eq!(read, image);
        });
    }

    #[test]
    fn a_see_through_png_reads_back_premultiplied() {
        with_com(|| {
            let image = picture::premultiplied(4, 3, gradient(128).bgra().to_vec()).unwrap();
            let read = from_bytes(&image.encode_png().unwrap()).unwrap();
            assert_eq!((read.width(), read.height()), (4, 3));
            for (read, made) in read.bgra().iter().zip(image.bgra()) {
                assert!(read.abs_diff(*made) <= 1, "{read} {made}");
            }
        });
    }

    #[test]
    fn a_clipboard_bitmap_reads_back_as_the_image_put_there() {
        with_com(|| {
            let image = gradient(u8::MAX);
            let Contents::Image { dib, .. } = Contents::image(&image, Arc::new(Vec::new())) else {
                panic!("an image goes on the clipboard as an image");
            };
            let read = from_bytes(&copy_of(Kind::Bitmap, &dib).unwrap()).unwrap();
            assert_eq!(read, image);
        });
    }

    #[test]
    fn a_clipboard_bitmap_with_its_alpha_left_at_zero_reads_back_opaque() {
        with_com(|| {
            let image = gradient(u8::MAX);
            let Contents::Image { mut dib, .. } = Contents::image(&image, Arc::new(Vec::new()))
            else {
                panic!("an image goes on the clipboard as an image");
            };
            for pixel in dib[size_of::<BITMAPV5HEADER>()..].chunks_mut(4) {
                pixel[3] = 0;
            }
            let read = from_bytes(&copy_of(Kind::Bitmap, &dib).unwrap()).unwrap();
            assert_eq!(read, image);
        });
    }

    #[test]
    fn a_png_with_no_visible_pixel_reads_back_see_through_and_saves_so() {
        with_com(|| {
            let clear = Image::from_bgra(4, 3, vec![0; 48]).unwrap();
            let read = from_bytes(&clear.encode_png().unwrap()).unwrap();
            assert_eq!(read, clear);
            assert!(!read.is_opaque());
            let again = from_bytes(&read.encode_png().unwrap()).unwrap();
            assert_eq!(again, clear);
        });
    }

    #[test]
    fn a_clipboard_image_larger_than_opens_is_refused_from_its_header() {
        let too_large: Result<Vec<u8>, OpenError> = Err(OpenError::TooLarge {
            width: 100_000,
            height: 100_000,
        });
        let mut dib = vec![0; 40];
        dib.splice(0..4, 40u32.to_le_bytes());
        dib.splice(4..8, 100_000i32.to_le_bytes());
        dib.splice(8..12, 100_000i32.to_le_bytes());
        dib.splice(14..16, 32u16.to_le_bytes());
        assert_eq!(copy_of(Kind::Bitmap, &dib), too_large);
        let mut png = gradient(u8::MAX).encode_png().unwrap();
        png.splice(16..20, 100_000u32.to_be_bytes());
        png.splice(20..24, 100_000u32.to_be_bytes());
        assert_eq!(copy_of(Kind::Png, &png), too_large);
    }

    #[test]
    fn a_clipboard_image_that_may_open_is_copied_whole() {
        let png = gradient(u8::MAX).encode_png().unwrap();
        assert_eq!(copy_of(Kind::Png, &png), Ok(png.clone()));
        assert_eq!(copy_of(Kind::Png, b"no header"), Ok(b"no header".to_vec()));
        assert!(matches!(
            copy_of(Kind::Bitmap, &[0; 20]),
            Err(OpenError::Unreadable(_))
        ));
    }

    #[test]
    fn bytes_that_are_no_image_cannot_be_read() {
        with_com(|| {
            let read = from_bytes(b"not an image at all");
            assert!(matches!(read, Err(OpenError::Unreadable(_))), "{read:?}");
        });
    }

    #[test]
    fn an_imaging_code_means_unreadable_and_any_other_keeps_windows_text() {
        let component_missing = Error::from(HRESULT(0x8898_2F50_u32 as i32));
        assert!(matches!(
            open_error(component_missing),
            OpenError::Unreadable(_)
        ));
        assert!(matches!(
            open_error(Error::from(E_ACCESSDENIED)),
            OpenError::Other(_)
        ));
    }
}
