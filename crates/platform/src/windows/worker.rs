//! The threads that encode screenshots to PNG and save them away from the platform thread, and
//! report each outcome back to it.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use bediz_core::delivery::{DeliveryError, SaveError};
use bediz_core::file_name::LocalTime;
use bediz_core::image::Image;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::clipboard::Contents;
use super::runtime::WM_OUTCOMES;
use super::save::{self, FileId, SavedAs};

const WAITING_TO_ENCODE: usize = 2;
const WAITING_TO_SAVE: usize = 4;
const INTERNAL_ERROR: &str = "an internal error was caught";

pub struct Job {
    pub id: u64,
    pub image: Image,
    pub copy: Option<u64>,
    pub save: Option<Target>,
    pub time: LocalTime,
}

pub enum Target {
    Folder {
        folder: Option<PathBuf>,
        stem: String,
    },
    Chosen {
        path: PathBuf,
        replace: Option<FileId>,
    },
}

pub enum Outcome {
    Encoded {
        job: u64,
        ticket: u64,
        contents: Contents,
    },
    Saved {
        job: u64,
        result: Result<(), SaveError>,
    },
    Appeared {
        job: u64,
        png: Png,
        path: PathBuf,
        time: LocalTime,
    },
    Failed {
        job: u64,
        error: DeliveryError,
    },
}

pub type Png = Arc<Vec<u8>>;

pub struct Save {
    pub job: u64,
    pub png: Png,
    pub target: Target,
    pub time: LocalTime,
}

type Outcomes = Arc<Mutex<Vec<Outcome>>>;

pub struct Worker {
    jobs: Option<SyncSender<Job>>,
    saves: Option<SyncSender<Save>>,
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
                for save in waiting_saves {
                    let job = save.job;
                    if catch_unwind(AssertUnwindSafe(|| save_one(save, &save_reply))).is_err() {
                        save_reply.send(Outcome::Saved {
                            job,
                            result: Err(SaveError::Unexpected(INTERNAL_ERROR.to_owned())),
                        });
                    }
                }
                if com {
                    unsafe { CoUninitialize() };
                }
            })?;
        let own_saves = saves.clone();
        let (jobs, waiting_jobs) = mpsc::sync_channel::<Job>(WAITING_TO_ENCODE);
        let encoder = std::thread::Builder::new()
            .name("bediz-encode".to_owned())
            .spawn(move || {
                for job in waiting_jobs {
                    let id = job.id;
                    if catch_unwind(AssertUnwindSafe(|| encode(job, &reply, &saves))).is_err() {
                        reply.send(Outcome::Failed {
                            job: id,
                            error: DeliveryError::Unexpected(INTERNAL_ERROR.to_owned()),
                        });
                    }
                }
            })?;
        Ok(Self {
            jobs: Some(jobs),
            saves: Some(own_saves),
            outcomes,
            threads: vec![encoder, saver],
        })
    }

    pub fn save(&self, save: Save) -> Result<(), SaveError> {
        match &self.saves {
            Some(saves) => queue_save(saves, save),
            None => Err(save_stopped()),
        }
    }

    pub fn send(&self, job: Job) -> Result<(), DeliveryError> {
        let Some(jobs) = &self.jobs else {
            return Err(stopped());
        };
        jobs.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) => DeliveryError::Busy,
            TrySendError::Disconnected(_) => stopped(),
        })
    }

    pub fn take_outcomes(&self) -> Vec<Outcome> {
        std::mem::take(&mut *self.outcomes.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn finish(mut self) -> Vec<Outcome> {
        self.stop();
        self.take_outcomes()
    }

    fn stop(&mut self) {
        self.jobs = None;
        self.saves = None;
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
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

const STOPPED: &str = "the threads that encode and save are not running";

pub fn stopped() -> DeliveryError {
    DeliveryError::Unexpected(STOPPED.to_owned())
}

pub fn save_stopped() -> SaveError {
    SaveError::Unexpected(STOPPED.to_owned())
}

fn encode(job: Job, reply: &Reply, saves: &SyncSender<Save>) {
    let Job {
        id,
        image,
        copy,
        save,
        time,
    } = job;
    let png = match image.encode_png() {
        Ok(png) => Arc::new(png),
        Err(error) => {
            reply.send(Outcome::Failed {
                job: id,
                error: DeliveryError::Unexpected(error.to_string()),
            });
            return;
        }
    };
    let (to_copy, to_save) = match (copy, save) {
        (Some(ticket), Some(target)) => (Some((ticket, Arc::clone(&png))), Some((target, png))),
        (Some(ticket), None) => (Some((ticket, png)), None),
        (None, Some(target)) => (None, Some((target, png))),
        (None, None) => (None, None),
    };
    if let Some((ticket, png)) = to_copy {
        reply.send(Outcome::Encoded {
            job: id,
            ticket,
            contents: Contents::image(&image, png),
        });
    }
    drop(image);
    if let Some((target, png)) = to_save {
        let save = Save {
            job: id,
            png,
            target,
            time,
        };
        if let Err(error) = queue_save(saves, save) {
            reply.send(Outcome::Saved {
                job: id,
                result: Err(error),
            });
        }
    }
}

fn queue_save(saves: &SyncSender<Save>, save: Save) -> Result<(), SaveError> {
    saves.try_send(save).map_err(|error| match error {
        TrySendError::Full(_) => SaveError::Busy,
        TrySendError::Disconnected(_) => save_stopped(),
    })
}

fn save_one(save: Save, reply: &Reply) {
    let Save {
        job,
        png,
        target,
        time,
    } = save;
    let outcome = match target {
        Target::Folder { folder, stem } => Outcome::Saved {
            job,
            result: save::folder(folder.as_deref())
                .and_then(|folder| save::save_screenshot(&folder, &stem, &png))
                .map(drop),
        },
        Target::Chosen { path, replace } => match save::save_chosen(&path, &png, replace) {
            Ok(SavedAs::NameTaken) => Outcome::Appeared {
                job,
                png,
                path,
                time,
            },
            saved => Outcome::Saved {
                job,
                result: saved.map(drop),
            },
        },
    };
    reply.send(outcome);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bediz_core::delivery::SaveError;
    use bediz_core::file_name::LocalTime;
    use bediz_core::image::Image;
    use windows::Win32::Foundation::HWND;

    use super::{Job, Outcome, Target, Worker};

    fn copy(id: u64) -> Job {
        Job {
            id,
            image: Image::blank(64, 48).unwrap(),
            copy: Some(id),
            save: None,
            time: LocalTime {
                year: 2026,
                month: 1,
                day: 2,
                hour: 3,
                minute: 4,
                second: 5,
            },
        }
    }

    #[test]
    fn finishing_waits_for_every_accepted_job() {
        let worker = Worker::start(HWND::default()).unwrap();
        worker.send(copy(1)).unwrap();
        worker.send(copy(2)).unwrap();
        let encoded: Vec<u64> = worker
            .finish()
            .into_iter()
            .map(|outcome| match outcome {
                Outcome::Encoded { job, ticket, .. } => {
                    assert_eq!(job, ticket);
                    job
                }
                _ => panic!("only copies were sent"),
            })
            .collect();
        assert_eq!(encoded, [1, 2]);
    }

    #[test]
    fn finishing_waits_for_the_save_of_an_accepted_job_too() {
        let worker = Worker::start(HWND::default()).unwrap();
        let job = Job {
            save: Some(Target::Folder {
                folder: Some(PathBuf::from("Pictures")),
                stem: "Bediz".to_owned(),
            }),
            ..copy(1)
        };
        worker.send(job).unwrap();
        let outcomes = worker.finish();
        assert!(
            outcomes
                .iter()
                .any(|outcome| matches!(outcome, Outcome::Encoded { job: 1, .. }))
        );
        assert!(outcomes.iter().any(|outcome| matches!(
            outcome,
            Outcome::Saved {
                job: 1,
                result: Err(SaveError::FolderMissing)
            }
        )));
    }
}
