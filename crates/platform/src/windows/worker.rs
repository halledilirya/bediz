use std::ffi::c_void;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use bediz_core::delivery::SaveError;
use bediz_core::file_name::LocalTime;
use bediz_core::geometry::Rect;
use bediz_core::image::Image;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::clipboard::Contents;
use super::runtime::WM_OUTCOMES;
use super::save;
use crate::event::Failure;

const WAITING_TO_ENCODE: usize = 2;
const WAITING_TO_SAVE: usize = 4;

pub struct Job {
    pub image: Image,
    pub rect: Rect,
    pub copy: Option<u64>,
    pub save: bool,
    pub time: LocalTime,
}

pub enum Outcome {
    Encoded {
        ticket: u64,
        rect: Rect,
        width: u32,
        height: u32,
        contents: Contents,
    },
    Saved {
        rect: Rect,
        width: u32,
        height: u32,
    },
    Failed(Failure),
}

struct Save {
    png: Vec<u8>,
    rect: Rect,
    width: u32,
    height: u32,
    time: LocalTime,
}

type Outcomes = Arc<Mutex<Vec<Outcome>>>;

pub struct Worker {
    jobs: Option<SyncSender<Job>>,
    outcomes: Outcomes,
    threads: Vec<JoinHandle<()>>,
}

impl Worker {
    pub fn start(window: HWND) -> std::io::Result<Self> {
        let outcomes: Outcomes = Arc::new(Mutex::new(Vec::new()));
        let reply = Reply {
            outcomes: Arc::clone(&outcomes),
            window: window.0 as isize,
        };
        let (saves, waiting_saves) = mpsc::sync_channel::<Save>(WAITING_TO_SAVE);
        let save_reply = reply.clone();
        let saver = std::thread::Builder::new()
            .name("bediz-save".to_owned())
            .spawn(move || {
                let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                for job in waiting_saves {
                    save_one(job, &save_reply);
                }
                if com {
                    unsafe { CoUninitialize() };
                }
            })?;
        let (jobs, waiting_jobs) = mpsc::sync_channel::<Job>(WAITING_TO_ENCODE);
        let encoder = std::thread::Builder::new()
            .name("bediz-encode".to_owned())
            .spawn(move || {
                for job in waiting_jobs {
                    encode(job, &reply, &saves);
                }
            })?;
        Ok(Self {
            jobs: Some(jobs),
            outcomes,
            threads: vec![encoder, saver],
        })
    }

    pub fn send(&self, job: Job) -> Result<(), Failure> {
        let Some(jobs) = &self.jobs else {
            return Err(stopped());
        };
        jobs.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) => Failure::Busy,
            TrySendError::Disconnected(_) => stopped(),
        })
    }

    pub fn take_outcomes(&self) -> Vec<Outcome> {
        std::mem::take(&mut *self.outcomes.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.jobs = None;
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[derive(Clone)]
struct Reply {
    outcomes: Outcomes,
    window: isize,
}

impl Reply {
    fn send(&self, outcome: Outcome) {
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(outcome);
        let window = HWND(self.window as *mut c_void);
        let _ = unsafe { PostMessageW(Some(window), WM_OUTCOMES, WPARAM(0), LPARAM(0)) };
    }
}

fn stopped() -> Failure {
    Failure::Encode("the threads that encode and save are not running".to_owned())
}

fn encode(job: Job, reply: &Reply, saves: &SyncSender<Save>) {
    let Job {
        image,
        rect,
        copy,
        save,
        time,
    } = job;
    let (width, height) = (image.width(), image.height());
    let png = match image.encode_png() {
        Ok(png) => png,
        Err(error) => {
            reply.send(Outcome::Failed(Failure::Encode(error.to_string())));
            return;
        }
    };
    let (to_copy, to_save) = match (copy, save) {
        (Some(ticket), true) => (Some((ticket, png.clone())), Some(png)),
        (Some(ticket), false) => (Some((ticket, png)), None),
        (None, true) => (None, Some(png)),
        (None, false) => (None, None),
    };
    if let Some((ticket, png)) = to_copy {
        reply.send(Outcome::Encoded {
            ticket,
            rect,
            width,
            height,
            contents: Contents::image(&image, png),
        });
    }
    drop(image);
    if let Some(png) = to_save {
        let queued = saves.try_send(Save {
            png,
            rect,
            width,
            height,
            time,
        });
        let refused = match queued {
            Ok(()) => None,
            Err(TrySendError::Full(_)) => Some(SaveError::Busy),
            Err(TrySendError::Disconnected(_)) => Some(SaveError::Other(stopped().to_string())),
        };
        if let Some(error) = refused {
            reply.send(Outcome::Failed(Failure::Save(error)));
        }
    }
}

fn save_one(job: Save, reply: &Reply) {
    let saved = save::screenshots_folder()
        .and_then(|folder| save::save_screenshot(&folder, &job.time, &job.png));
    reply.send(match saved {
        Ok(_) => Outcome::Saved {
            rect: job.rect,
            width: job.width,
            height: job.height,
        },
        Err(error) => Outcome::Failed(Failure::Save(error)),
    });
}
