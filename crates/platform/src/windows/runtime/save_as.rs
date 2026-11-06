use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use bediz_core::delivery::{Delivery, DeliveryError, SaveError};
use bediz_core::file_name::{self, LocalTime};
use bediz_core::geometry::Rect;
use bediz_core::image::Image;
use bediz_core::notice::Notice;
use windows::Win32::Foundation::HWND;

use super::Runtime;
use crate::windows::clock;
use crate::windows::foreground;
use crate::windows::save_dialog::{self, Suggestion};
use crate::windows::worker::{self, Save, Target};

pub(super) struct PendingSaveAs {
    dialog: u64,
    rect: Rect,
    unsaved: Unsaved,
    time: LocalTime,
    previous: HWND,
}

enum Unsaved {
    Image(Image),
    Encoded { job: u64, png: Vec<u8> },
}

pub(super) struct Retry {
    job: u64,
    png: Vec<u8>,
    path: PathBuf,
    time: LocalTime,
}

impl Runtime {
    pub(super) fn save_as(&mut self) {
        if self.save_as.is_some() {
            return;
        }
        let Some((rect, read)) = self.read_selection() else {
            return;
        };
        let previous = self.close_overlay(false);
        let image = match read {
            Ok(image) => image,
            Err(error) => {
                foreground::give_back(previous);
                self.notify(vec![Notice::DeliveryFailed(error)], rect);
                return;
            }
        };
        let time = clock::local_time();
        let stem = self.file_name.stem(&time, rect.width(), rect.height());
        let suggestion = Suggestion {
            name: file_name::png(&stem, 1),
            folder: None,
            usual_folder: self.save_folder.clone(),
        };
        let pending = PendingSaveAs {
            dialog: 0,
            rect,
            unsaved: Unsaved::Image(image),
            time,
            previous,
        };
        if let Err(error) = self.ask(pending, suggestion) {
            foreground::give_back(previous);
            let error = DeliveryError::Other(error.to_string());
            self.notify(vec![Notice::DeliveryFailed(error)], rect);
        }
    }

    pub(super) fn ask_again(&mut self, job: u64, png: Vec<u8>, path: PathBuf, time: LocalTime) {
        if self.save_as.is_some() {
            self.retries.push_back(Retry {
                job,
                png,
                path,
                time,
            });
            return;
        }
        let suggestion = Suggestion {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            folder: path.parent().map(Path::to_path_buf),
            usual_folder: None,
        };
        let pending = PendingSaveAs {
            dialog: 0,
            rect: self.near_of(job).unwrap_or_default(),
            unsaved: Unsaved::Encoded { job, png },
            time,
            previous: HWND::default(),
        };
        if let Err(error) = self.ask(pending, suggestion) {
            let error = SaveError::Other(error.to_string());
            self.update(job, |report| report.saved(Err(error)));
        }
    }

    pub(super) fn on_save_as_answer(&mut self) {
        let answer = self
            .save_as_answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let (Some(answer), Some(pending)) = (answer, self.save_as.take()) else {
            return;
        };
        if answer.job != pending.dialog {
            self.save_as = Some(pending);
            return;
        }
        if self.session.is_none() {
            foreground::give_back_from_us(pending.previous);
        }
        let chosen = answer.chosen.map(|chosen| {
            chosen.map(|chosen| Target::Chosen {
                path: chosen.path,
                replace: chosen.replace,
            })
        });
        match (chosen, pending.unsaved) {
            (Ok(Some(target)), Unsaved::Image(image)) => self.send_job(
                pending.rect,
                Ok(image),
                Delivery::SAVE,
                Some(target),
                pending.time,
            ),
            (Ok(Some(target)), Unsaved::Encoded { job, png }) => {
                let save = Save {
                    job,
                    png,
                    target,
                    time: pending.time,
                };
                let queued = match &self.worker {
                    Some(worker) => worker.save(save),
                    None => Err(SaveError::Other(worker::stopped().to_string())),
                };
                if let Err(error) = queued {
                    self.update(job, |report| report.saved(Err(error)));
                }
            }
            (Ok(None), Unsaved::Image(_)) => {}
            (Ok(None), Unsaved::Encoded { job, .. }) => self.forget(job),
            (Err(error), Unsaved::Image(_)) => {
                let error = SaveError::Other(error.message());
                self.notify(vec![Notice::SaveFailed(error)], pending.rect);
            }
            (Err(error), Unsaved::Encoded { job, .. }) => {
                let error = SaveError::Other(error.message());
                self.update(job, |report| report.saved(Err(error)));
            }
        }
        while self.save_as.is_none() {
            let Some(retry) = self.retries.pop_front() else {
                break;
            };
            self.ask_again(retry.job, retry.png, retry.path, retry.time);
        }
    }

    fn ask(&mut self, mut pending: PendingSaveAs, suggestion: Suggestion) -> std::io::Result<()> {
        self.deliveries = self.deliveries.wrapping_add(1);
        pending.dialog = self.deliveries;
        save_dialog::ask(
            self.window.0,
            &self.save_as_answers,
            pending.dialog,
            suggestion,
        )?;
        self.save_as = Some(pending);
        Ok(())
    }
}
