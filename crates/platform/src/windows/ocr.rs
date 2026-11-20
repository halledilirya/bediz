//! Reads the text in an image with Windows' own recognizer, and its QR codes alongside, on threads
//! of their own, in the language chosen or the first of the user's Windows languages it can read,
//! and lists those it can read.

use std::ffi::c_void;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use bediz_core::image::Image;
use bediz_core::ocr::{self, Bounds, Page, Placement, Script, Word};
use bediz_core::qr::{self, Scan};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Win32::Foundation::{E_POINTER, E_UNEXPECTED, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::WinRT::IMemoryBufferByteAccess;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{Error, HSTRING, Interface};

use super::runtime::WM_TEXT;

const LARGEST_WHEN_UNKNOWN: u32 = 4_096;

static FIRST_LISTED: OnceLock<Vec<String>> = OnceLock::new();

pub enum ReadError {
    NoLanguage,
    LanguageMissing,
    Failed(String),
    NeedsRestart(String),
}

pub struct Reading {
    pub placement: Placement,
    pub lines: Vec<Vec<Word>>,
    pub language: String,
    pub passes: u32,
    pub took: Duration,
}

pub struct Looked {
    pub scan: Scan,
    pub took: Duration,
}

pub struct Read {
    pub request: u64,
    pub result: Result<Reading, ReadError>,
    pub codes: Result<Looked, String>,
}

pub type Readings = Arc<Mutex<Option<Read>>>;

pub fn read(
    window: HWND,
    readings: &Readings,
    request: u64,
    image: Image,
    language: Option<String>,
) -> io::Result<()> {
    let readings = Arc::clone(readings);
    let window = window.0 as isize;
    std::thread::Builder::new()
        .name("bediz-ocr".to_owned())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let reading = || read_and_look(&image, language.as_deref());
            let (result, codes) = catch_unwind(AssertUnwindSafe(reading)).unwrap_or_else(|_| {
                let caught = "an internal error was caught while reading the text";
                (
                    Err(ReadError::Failed(caught.to_owned())),
                    Err(caught.to_owned()),
                )
            });
            if com {
                unsafe { CoUninitialize() };
            }
            *readings.lock().unwrap_or_else(PoisonError::into_inner) = Some(Read {
                request,
                result,
                codes,
            });
            let window = HWND(window as *mut c_void);
            let _ = unsafe { PostMessageW(Some(window), WM_TEXT, WPARAM(0), LPARAM(0)) };
        })?;
    Ok(())
}

fn read_and_look(
    image: &Image,
    chosen: Option<&str>,
) -> (Result<Reading, ReadError>, Result<Looked, String>) {
    std::thread::scope(|scope| {
        let looking = std::thread::Builder::new()
            .name("bediz-qr".to_owned())
            .spawn_scoped(scope, || {
                let started = Instant::now();
                let scan = qr::find(image);
                Looked {
                    scan,
                    took: started.elapsed(),
                }
            });
        let reading = || recognize(image, chosen, first_listed());
        let result = catch_unwind(AssertUnwindSafe(reading)).unwrap_or_else(|_| {
            Err(ReadError::Failed(
                "an internal error was caught while reading the text".to_owned(),
            ))
        });
        let codes = match looking {
            Ok(looking) => looking
                .join()
                .map_err(|_| "an internal error was caught while looking for QR codes".to_owned()),
            Err(error) => Err(format!(
                "the thread that looks for QR codes could not start: {error}"
            )),
        };
        (result, codes)
    })
}

pub fn text_languages() -> io::Result<Vec<String>> {
    std::thread::Builder::new()
        .name("bediz-ocr-languages".to_owned())
        .spawn(|| {
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let listed =
                recognizer_languages().map_err(|error| io::Error::other(described(&error)));
            if com {
                unsafe { CoUninitialize() };
            }
            listed
        })?
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("the text languages could not be listed")))
}

fn recognizer_languages() -> windows::core::Result<Vec<String>> {
    let languages = OcrEngine::AvailableRecognizerLanguages()?;
    let listed = (0..languages.Size()?)
        .map(|at| Ok(languages.GetAt(at)?.LanguageTag()?.to_string_lossy()))
        .collect::<windows::core::Result<Vec<_>>>()?;
    FIRST_LISTED.get_or_init(|| listed.clone());
    Ok(listed)
}

fn first_listed() -> Option<&'static [String]> {
    if FIRST_LISTED.get().is_none() {
        recognizer_languages().ok()?;
    }
    FIRST_LISTED.get().map(Vec::as_slice)
}

fn after_install(error: ReadError, first: Option<&[String]>, asked: Option<&str>) -> ReadError {
    let installed = first
        .zip(recognizer_languages().ok())
        .is_some_and(|(first, now)| ocr::installed_since(first, &now, asked));
    match error {
        ReadError::Failed(reason) if installed => ReadError::NeedsRestart(reason),
        ReadError::NoLanguage | ReadError::LanguageMissing if installed => {
            ReadError::NeedsRestart("Windows made no recognizer".to_owned())
        }
        other => other,
    }
}

fn recognize(
    image: &Image,
    chosen: Option<&str>,
    first: Option<&[String]>,
) -> Result<Reading, ReadError> {
    let started = Instant::now();
    let engine = match chosen {
        Some(tag) => chosen_engine(tag),
        None => engine(),
    }
    .map_err(|error| after_install(error, first, chosen))?;
    let language = engine
        .RecognizerLanguage()
        .and_then(|language| language.LanguageTag())
        .map(|tag| tag.to_string_lossy())
        .unwrap_or_default();
    let tried = chosen.unwrap_or(&language);
    let (lines, placement, passes) =
        passes(&engine, image).map_err(|error| after_install(error, first, Some(tried)))?;
    Ok(Reading {
        placement,
        lines: ocr::rows(lines, Script::of(&language)),
        language,
        passes,
        took: started.elapsed(),
    })
}

fn passes(
    engine: &OcrEngine,
    image: &Image,
) -> Result<(Vec<Vec<Word>>, Placement, u32), ReadError> {
    let largest = OcrEngine::MaxImageDimension().unwrap_or(LARGEST_WHEN_UNKNOWN);
    let (first, placed_first) = pass(engine, image, ocr::first_scale(image, largest))?;
    Ok(match ocr::better_scale(&first, &placed_first, largest) {
        None => (first, placed_first, 1),
        Some(scale) => {
            let (again, placed_again) = pass(engine, image, scale)?;
            if ocr::prefers_second(&first, &again) {
                (again, placed_again, 2)
            } else {
                (first, placed_first, 2)
            }
        }
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
    let mut failure = None;
    for at in 0..languages.Size().map_err(failed)? {
        let language = languages.GetAt(at).map_err(failed)?;
        match OcrEngine::TryCreateFromLanguage(&language) {
            Ok(engine) => return Ok(engine),
            Err(error) if error.code().is_ok() => {}
            Err(error) => {
                failure.get_or_insert(error);
            }
        }
    }
    Err(failure.map_or(ReadError::NoLanguage, failed))
}

fn chosen_engine(tag: &str) -> Result<OcrEngine, ReadError> {
    let tag = HSTRING::from(tag);
    if !Language::IsWellFormed(&tag).map_err(failed)? {
        return Err(ReadError::LanguageMissing);
    }
    let language = Language::CreateLanguage(&tag).map_err(failed)?;
    match OcrEngine::TryCreateFromLanguage(&language) {
        Ok(engine) => Ok(engine),
        Err(error) if error.code().is_ok() => Err(ReadError::LanguageMissing),
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
    ReadError::Failed(described(&error))
}

fn described(error: &Error) -> String {
    format!("{} ({:#010x})", error.message(), error.code().0)
}
