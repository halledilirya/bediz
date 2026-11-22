//! The thread that packs a new pin's pixels into a PNG and unpacks them when a hidden pin is shown
//! again, one pin after another, so neither holds up the platform thread.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use bediz_core::image::{Image, ImageError};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::windows::runtime::WM_PIN_KEPT;
use crate::windows::worker::{INTERNAL_ERROR, Png};

enum Job {
    Pack(u64, Arc<Image>),
    Unpack(u64, Png),
}

pub(super) enum Kept {
    Packed(u64, Result<Png, String>),
    Unpacked(u64, Result<Arc<Image>, String>),
}

type Done = Arc<Mutex<Vec<Kept>>>;

#[derive(Default)]
pub(super) struct Keeper {
    jobs: Option<Sender<Job>>,
    done: Done,
}

impl Keeper {
    pub(super) fn pack(&mut self, window: HWND, id: u64, image: Arc<Image>) -> std::io::Result<()> {
        self.send(window, Job::Pack(id, image))
    }

    pub(super) fn unpack(&mut self, window: HWND, id: u64, png: Png) -> std::io::Result<()> {
        self.send(window, Job::Unpack(id, png))
    }

    pub(super) fn take(&self) -> Vec<Kept> {
        std::mem::take(&mut *self.done.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn send(&mut self, window: HWND, job: Job) -> std::io::Result<()> {
        if self.jobs.is_none() {
            self.jobs = Some(start(window, Arc::clone(&self.done))?);
        }
        let sent = self.jobs.as_ref().map(|jobs| jobs.send(job));
        if let Some(Ok(())) = sent {
            return Ok(());
        }
        self.jobs = None;
        Err(std::io::Error::other(
            "the thread that keeps the pins' pixels stopped",
        ))
    }
}

fn start(window: HWND, done: Done) -> std::io::Result<Sender<Job>> {
    let (jobs, waiting) = mpsc::channel::<Job>();
    let window = window.0 as isize;
    std::thread::Builder::new()
        .name("bediz-pin".to_owned())
        .spawn(move || {
            for job in waiting {
                let kept = match job {
                    Job::Pack(id, image) => {
                        Kept::Packed(id, guarded(move || image.encode_png().map(Arc::new)))
                    }
                    Job::Unpack(id, png) => {
                        Kept::Unpacked(id, guarded(move || Image::decode_png(&png).map(Arc::new)))
                    }
                };
                done.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(kept);
                let window = HWND(window as *mut c_void);
                let _ = unsafe { PostMessageW(Some(window), WM_PIN_KEPT, WPARAM(0), LPARAM(0)) };
            }
        })?;
    Ok(jobs)
}

fn guarded<T>(work: impl FnOnce() -> Result<T, ImageError>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work))
        .map_err(|_| INTERNAL_ERROR.to_owned())
        .and_then(|done| done.map_err(|error| error.to_string()))
}
