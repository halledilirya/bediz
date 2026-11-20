//! Reads the text in an image with Windows' own recognizer, on a thread of its own, in the first
//! of the user's Windows languages it can read.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use bediz_core::image::Image;
use bediz_core::ocr::{self, Bounds, Page, Placement, Script, Word};
use windows::Graphics::Imaging::{BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Win32::Foundation::{E_POINTER, E_UNEXPECTED, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::WinRT::IMemoryBufferByteAccess;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{Error, Interface};

use super::runtime::WM_TEXT;

const LARGEST_WHEN_UNKNOWN: u32 = 4_096;

pub enum ReadError {
    NoLanguage,
    Failed(String),
}

pub struct Reading {
    pub placement: Placement,
    pub lines: Vec<Vec<Word>>,
    pub language: String,
    pub passes: u32,
    pub took: Duration,
}

pub struct Read {
    pub request: u64,
    pub result: Result<Reading, ReadError>,
}

pub type Readings = Arc<Mutex<Option<Read>>>;

pub fn read(window: HWND, readings: &Readings, request: u64, image: Image) -> std::io::Result<()> {
    let readings = Arc::clone(readings);
    let window = window.0 as isize;
    std::thread::Builder::new()
        .name("bediz-ocr".to_owned())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let result = catch_unwind(AssertUnwindSafe(|| recognize(image))).unwrap_or_else(|_| {
                Err(ReadError::Failed(
                    "an internal error was caught while reading the text".to_owned(),
                ))
            });
            if com {
                unsafe { CoUninitialize() };
            }
            *readings.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(Read { request, result });
            let window = HWND(window as *mut c_void);
            let _ = unsafe { PostMessageW(Some(window), WM_TEXT, WPARAM(0), LPARAM(0)) };
        })?;
    Ok(())
}

fn recognize(image: Image) -> Result<Reading, ReadError> {
    let started = Instant::now();
    let engine = engine()?;
    let language = engine
        .RecognizerLanguage()
        .and_then(|language| language.LanguageTag())
        .map(|tag| tag.to_string_lossy())
        .unwrap_or_default();
    let largest = OcrEngine::MaxImageDimension().unwrap_or(LARGEST_WHEN_UNKNOWN);
    let (first, placed_first) = pass(&engine, &image, ocr::first_scale(&image, largest))?;
    let (lines, placement, passes) = match ocr::better_scale(&first, &placed_first, largest) {
        None => (first, placed_first, 1),
        Some(scale) => {
            let (again, placed_again) = pass(&engine, &image, scale)?;
            if ocr::prefers_second(&first, &again) {
                (again, placed_again, 2)
            } else {
                (first, placed_first, 2)
            }
        }
    };
    Ok(Reading {
        placement,
        lines: ocr::rows(lines, Script::of(&language)),
        language,
        passes,
        took: started.elapsed(),
    })
}

fn pass(
    engine: &OcrEngine,
    image: &Image,
    scale: f32,
) -> Result<(Vec<Vec<Word>>, Placement), ReadError> {
    let page = ocr::prepare(image, scale);
    let (lines, angle) = words(engine, &page).map_err(failed)?;
    Ok((lines, page.placement().turned(angle)))
}

// A recognizer the system cannot make comes back as an empty error, not as a failure.
fn engine() -> Result<OcrEngine, ReadError> {
    match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(engine) => return Ok(engine),
        Err(error) if error.code().is_ok() => {}
        Err(error) => return Err(failed(error)),
    }
    let languages = OcrEngine::AvailableRecognizerLanguages().map_err(failed)?;
    if languages.Size().map_err(failed)? == 0 {
        return Err(ReadError::NoLanguage);
    }
    let first = languages.GetAt(0).map_err(failed)?;
    match OcrEngine::TryCreateFromLanguage(&first) {
        Ok(engine) => Ok(engine),
        Err(error) if error.code().is_ok() => Err(ReadError::NoLanguage),
        Err(error) => Err(failed(error)),
    }
}

fn words(engine: &OcrEngine, page: &Page) -> windows::core::Result<(Vec<Vec<Word>>, f32)> {
    let bitmap = bitmap(page)?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let angle = result
        .TextAngle()
        .and_then(|angle| angle.Value())
        .map_or(0.0, |degrees| degrees as f32);
    let lines = result.Lines()?;
    let mut read = Vec::new();
    for line in 0..lines.Size()? {
        let words = lines.GetAt(line)?.Words()?;
        let mut found = Vec::new();
        for word in 0..words.Size()? {
            let word = words.GetAt(word)?;
            let rect = word.BoundingRect()?;
            found.push(Word {
                text: word.Text()?.to_string_lossy(),
                bounds: Bounds {
                    left: rect.X,
                    top: rect.Y,
                    width: rect.Width,
                    height: rect.Height,
                },
            });
        }
        read.push(found);
    }
    Ok((read, angle))
}

fn bitmap(page: &Page) -> windows::core::Result<SoftwareBitmap> {
    let side = |side: u32| i32::try_from(side).map_err(|_| Error::from(E_UNEXPECTED));
    let count = |count: i32| usize::try_from(count).map_err(|_| Error::from(E_UNEXPECTED));
    let (width, height) = page.placement().size();
    let bitmap = SoftwareBitmap::Create(BitmapPixelFormat::Gray8, side(width)?, side(height)?)?;
    let buffer = bitmap.LockBuffer(BitmapBufferAccessMode::Write)?;
    let plane = buffer.GetPlaneDescription(0)?;
    let reference = buffer.CreateReference()?;
    let access: IMemoryBufferByteAccess = reference.cast()?;
    let mut data = std::ptr::null_mut();
    let mut capacity = 0;
    unsafe { access.GetBuffer(&mut data, &mut capacity) }?;
    if data.is_null() {
        return Err(E_POINTER.into());
    }
    let bytes = unsafe { std::slice::from_raw_parts_mut(data, capacity as usize) };
    let start = count(plane.StartIndex)?;
    if let Some(plane_bytes) = bytes.get_mut(start..) {
        page.paint(plane_bytes, count(plane.Stride)?);
    }
    reference.Close()?;
    buffer.Close()?;
    Ok(bitmap)
}

fn failed(error: Error) -> ReadError {
    ReadError::Failed(error.message())
}
