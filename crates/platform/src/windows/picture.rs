//! Opens an image to mark up, from a file the user picks or from the clipboard, and reads it with
//! the Windows Imaging Component, upright, in sRGB and premultiplied, on a thread of its own.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bediz_core::image::Image;
use bediz_core::picture::{self, OpenError};
use windows::Win32::Foundation::{ERROR_CANCELLED, GENERIC_READ, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatBmp, GUID_WICPixelFormat32bppBGRA,
    IWICBitmapDecoder, IWICBitmapFrameDecode, IWICBitmapSource, IWICColorContext,
    IWICColorTransform, IWICImagingFactory, IWICPalette, WICBitmapDitherTypeNone,
    WICBitmapPaletteTypeCustom, WICColorContextExifColorSpace, WICColorContextProfile,
    WICDecodeMetadataCacheOnDemand,
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
use windows::core::{Error, HSTRING, Interface, PCWSTR, w};

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
const EXIF_SRGB: u32 = 1;
const EXIF_ADOBE_RGB: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Colours {
    Profile,
    Exif(u32),
    Unknown,
}

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
    let transform = in_srgb(factory, &frame, &converter);
    let source: &IWICBitmapSource = match &transform {
        Some(transform) => transform,
        None => &converter,
    };
    let (mut width, mut height) = (0, 0);
    unsafe { source.GetSize(&mut width, &mut height) }.map_err(open_error)?;
    picture::check_size(width, height)?;
    let too_large = || OpenError::TooLarge { width, height };
    let stride = width.checked_mul(4).ok_or_else(too_large)?;
    let length = (stride as usize)
        .checked_mul(height as usize)
        .ok_or_else(too_large)?;
    let mut bgra = Vec::new();
    bgra.try_reserve_exact(length).map_err(|_| too_large())?;
    bgra.resize(length, 0);
    unsafe { source.CopyPixels(std::ptr::null(), stride, &mut bgra) }.map_err(open_error)?;
    if unsafe { decoder.GetContainerFormat() }.is_ok_and(|format| format == GUID_ContainerFormatBmp)
    {
        picture::fill_unused_alpha(&mut bgra);
    }
    let image = picture::premultiplied(width, height, bgra)
        .map_err(|error| OpenError::Unexpected(error.to_string()))?;
    Ok(picture::upright(&image, orientation(&frame)).unwrap_or(image))
}

// The overlay shows sRGB and the PNG is saved as sRGB, so an image's own profile is applied.
fn in_srgb(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
    converted: &IWICBitmapSource,
) -> Option<IWICColorTransform> {
    let profile = profile_of(factory, frame)?;
    let sources: [&IWICBitmapSource; 2] = [frame, converted];
    to_srgb(factory, &sources, &profile)
}

fn profile_of(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Option<IWICColorContext> {
    let mut count = 0;
    // Microsoft's page counts the contexts with a null array; an empty slice's pointer is not null.
    let counted = unsafe {
        (frame.vtable().GetColorContexts)(frame.as_raw(), 0, std::ptr::null_mut(), &mut count)
    };
    if counted.is_err() || count == 0 {
        return None;
    }
    let mut contexts = (0..count)
        .map(|_| unsafe { factory.CreateColorContext() }.ok())
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    unsafe { frame.GetColorContexts(&mut contexts, &mut count) }.ok()?;
    let contexts: Vec<IWICColorContext> = contexts.into_iter().flatten().collect();
    let colours: Vec<Colours> = contexts.iter().map(colours_of).collect();
    contexts.into_iter().nth(to_convert(&colours)?)
}

fn colours_of(context: &IWICColorContext) -> Colours {
    match unsafe { context.GetType() } {
        Ok(kind) if kind == WICColorContextProfile => Colours::Profile,
        Ok(kind) if kind == WICColorContextExifColorSpace => {
            unsafe { context.GetExifColorSpace() }.map_or(Colours::Unknown, Colours::Exif)
        }
        _ => Colours::Unknown,
    }
}

// A profile says the most; of Exif's colour spaces only Adobe RGB is not sRGB.
fn to_convert(colours: &[Colours]) -> Option<usize> {
    colours
        .iter()
        .position(|colours| *colours == Colours::Profile)
        .or_else(|| {
            colours
                .iter()
                .position(|colours| *colours == Colours::Exif(EXIF_ADOBE_RGB))
        })
}

// The transform takes only some pixel formats, so a frame in another goes through the converter.
fn to_srgb(
    factory: &IWICImagingFactory,
    sources: &[&IWICBitmapSource],
    profile: &IWICColorContext,
) -> Option<IWICColorTransform> {
    let srgb = unsafe { factory.CreateColorContext() }.ok()?;
    unsafe { srgb.InitializeFromExifColorSpace(EXIF_SRGB) }.ok()?;
    sources.iter().find_map(|source| {
        let transform = unsafe { factory.CreateColorTransformer() }.ok()?;
        unsafe { transform.Initialize(*source, profile, &srgb, &GUID_WICPixelFormat32bppBGRA) }
            .ok()?;
        Some(transform)
    })
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
    use windows::Win32::Graphics::Imaging::{
        GUID_ContainerFormatPng, IWICBitmapFrameEncode, WICBitmapEncoderNoCache,
    };
    use windows::Win32::System::Com::{
        COINIT_MULTITHREADED, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
    };
    use windows::Win32::UI::Shell::SHCreateMemStream;
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

    fn adobe_rgb_profile() -> Vec<u8> {
        let fixed = |value: f64| ((value * 65536.0).round() as i32).to_be_bytes();
        let xyz = |x: f64, y: f64, z: f64| {
            [b"XYZ \0\0\0\0".as_slice(), &fixed(x), &fixed(y), &fixed(z)].concat()
        };
        let d50 = xyz(0.9642, 1.0, 0.8249);
        let description = [
            b"desc\0\0\0\0".as_slice(),
            &5u32.to_be_bytes(),
            b"test\0",
            &[0u8; 78],
        ];
        let gamma = [b"curv\0\0\0\0".as_slice(), &1u32.to_be_bytes(), &[2u8, 51]];
        let elements: [(&[&[u8; 4]], Vec<u8>); 7] = [
            (&[b"desc"], description.concat()),
            (&[b"cprt"], b"text\0\0\0\0none\0".to_vec()),
            (&[b"wtpt"], d50.clone()),
            (&[b"rXYZ"], xyz(0.6097, 0.3111, 0.0195)),
            (&[b"gXYZ"], xyz(0.2053, 0.6257, 0.0609)),
            (&[b"bXYZ"], xyz(0.1492, 0.0632, 0.7446)),
            (&[b"rTRC", b"gTRC", b"bTRC"], gamma.concat()),
        ];
        let tags: usize = elements.iter().map(|(names, _)| names.len()).sum();
        let start = 128 + 4 + tags * 12;
        let (mut table, mut data) = ((tags as u32).to_be_bytes().to_vec(), Vec::new());
        for (names, element) in &elements {
            for name in *names {
                table.extend_from_slice(*name);
                table.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
                table.extend_from_slice(&(element.len() as u32).to_be_bytes());
            }
            data.extend_from_slice(element);
            data.resize(data.len().next_multiple_of(4), 0);
        }
        let mut header = vec![0; 128];
        header[..4].copy_from_slice(&((start + data.len()) as u32).to_be_bytes());
        header[8..12].copy_from_slice(&[2, 0x10, 0, 0]);
        header[12..24].copy_from_slice(b"mntrRGB XYZ ");
        header[36..40].copy_from_slice(b"acsp");
        header[68..80].copy_from_slice(&d50[8..]);
        [header, table, data].concat()
    }

    fn png_with_profile(bgra: &[u8], width: u32, profile: &[u8]) -> Vec<u8> {
        let factory = factory().unwrap();
        let context = unsafe { factory.CreateColorContext() }.unwrap();
        unsafe { context.InitializeFromMemory(profile) }.unwrap();
        let stream = unsafe { SHCreateMemStream(None) }.unwrap();
        let encoder =
            unsafe { factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()) }.unwrap();
        let (mut frame, mut options) = (None, None);
        let mut format = GUID_WICPixelFormat32bppBGRA;
        let height = bgra.len() as u32 / (width * 4);
        unsafe {
            encoder
                .Initialize(&stream, WICBitmapEncoderNoCache)
                .unwrap();
            encoder.CreateNewFrame(&mut frame, &mut options).unwrap();
            let frame: IWICBitmapFrameEncode = frame.unwrap();
            frame.Initialize(options.as_ref()).unwrap();
            frame.SetSize(width, height).unwrap();
            frame.SetPixelFormat(&mut format).unwrap();
            frame.SetColorContexts(&[Some(context)]).unwrap();
            frame.WritePixels(height, width * 4, bgra).unwrap();
            frame.Commit().unwrap();
            encoder.Commit().unwrap();
        }
        assert_eq!(format, GUID_WICPixelFormat32bppBGRA);
        let mut stat = STATSTG::default();
        unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }.unwrap();
        let mut png = vec![0u8; stat.cbSize as usize];
        let mut read = 0;
        unsafe {
            stream.Seek(0, STREAM_SEEK_SET, None).unwrap();
            stream
                .Read(
                    png.as_mut_ptr().cast(),
                    png.len() as u32,
                    Some(&raw mut read),
                )
                .ok()
                .unwrap();
        }
        assert_eq!(read as usize, png.len());
        png
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
    fn a_profile_or_adobe_rgb_is_converted_and_srgb_or_nothing_is_not() {
        assert_eq!(to_convert(&[]), None);
        assert_eq!(to_convert(&[Colours::Exif(EXIF_SRGB)]), None);
        assert_eq!(to_convert(&[Colours::Exif(0xFFFF), Colours::Unknown]), None);
        assert_eq!(to_convert(&[Colours::Exif(EXIF_ADOBE_RGB)]), Some(0));
        let both = [Colours::Exif(EXIF_ADOBE_RGB), Colours::Profile];
        assert_eq!(to_convert(&both), Some(1));
    }

    #[test]
    fn adobe_rgb_colours_are_brought_to_srgb() {
        with_com(|| {
            let factory = factory().unwrap();
            let pixels: [u8; 8] = [128, 128, 128, 255, 64, 128, 64, 255];
            let bitmap = unsafe {
                factory.CreateBitmapFromMemory(2, 1, &GUID_WICPixelFormat32bppBGRA, 8, &pixels)
            }
            .unwrap();
            let adobe = unsafe { factory.CreateColorContext() }.unwrap();
            unsafe { adobe.InitializeFromExifColorSpace(EXIF_ADOBE_RGB) }.unwrap();
            let source: &IWICBitmapSource = &bitmap;
            let transform = to_srgb(&factory, &[source], &adobe).unwrap();
            let mut read = [0u8; 8];
            unsafe { transform.CopyPixels(std::ptr::null(), 8, &mut read) }.unwrap();
            for level in &read[..3] {
                assert!(level.abs_diff(128) <= 3, "{read:?}");
            }
            assert!(read[6] < 32, "{read:?}");
            assert_eq!([read[3], read[7]], [255, 255]);
        });
    }

    #[test]
    fn a_png_carrying_an_adobe_rgb_profile_opens_in_srgb() {
        with_com(|| {
            let green = [64u8, 128, 64, 255].repeat(4);
            let png = png_with_profile(&green, 2, &adobe_rgb_profile());
            let factory = factory().unwrap();
            let stream = unsafe { factory.CreateStream() }.unwrap();
            unsafe { stream.InitializeFromMemory(&png) }.unwrap();
            let decoder = unsafe {
                factory.CreateDecoderFromStream(
                    &stream,
                    std::ptr::null(),
                    WICDecodeMetadataCacheOnDemand,
                )
            }
            .unwrap();
            let frame = unsafe { decoder.GetFrame(0) }.unwrap();
            let profile = profile_of(&factory, &frame).unwrap();
            assert_eq!(colours_of(&profile), Colours::Profile);
            let read = from_bytes(&png).unwrap();
            for pixel in read.bgra().chunks(4) {
                assert!(pixel[2] < 32 && pixel[3] == 255, "{pixel:?}");
            }
        });
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
