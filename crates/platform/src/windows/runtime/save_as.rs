//! Save as: asks where to save a capture, one dialog at a time with later ones waiting in line,
//! asks again when the chosen name was taken meanwhile, and keeps accepted saves through a quit.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use bediz_core::delivery::{Delivery, DeliveryError, SaveError};
use bediz_core::file_name::{self, LocalTime};
use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use windows::Win32::Foundation::HWND;

use super::Runtime;
use super::delivery::Origin;
use crate::windows::clock;
use crate::windows::foreground;
use crate::windows::save_dialog::{self, Suggestion};
use crate::windows::session::Session;
use crate::windows::worker::{self, Pixels, Png, Save, Target};

const IMAGES_WAITING: usize = 2;

pub(super) struct PendingSaveAs {
    dialog: u64,
    rect: Rect,
    origin: Origin,
    unsaved: Unsaved,
    time: LocalTime,
    previous: HWND,
}

pub(super) struct Waiting {
    pending: PendingSaveAs,
    suggestion: Suggestion,
}

enum Unsaved {
    Pixels(Pixels),
    Encoded { job: u64, png: Png, path: PathBuf },
}

impl Runtime {
    pub(super) fn save_as(&mut self) {
        if self.save_as.is_some() && !has_room(&self.waiting) {
            let Some(rect) = self.session.as_ref().and_then(Session::selected) else {
                return;
            };
            self.hide();
            self.notify(vec![Notice::SaveFailed(SaveError::Busy)], rect);
            return;
        }
        let Some((rect, read)) = self.read_selection() else {
            return;
        };
        let origin = self.origin();
        let previous = self.close_overlay(false);
        let image = match read {
            Ok(image) => image,
            Err(error) => {
                foreground::give_back(previous);
                self.notify(vec![Notice::DeliveryFailed(error)], rect);
                return;
            }
        };
        let size = (image.width(), image.height());
        if self.save_as.is_some() {
            foreground::give_back(previous);
        }
        let pixels = Pixels::Image(Arc::new(image));
        self.ask_to_save(rect, origin, pixels, size, previous);
    }

    pub(super) fn save_pin_as(&mut self, near: Rect, pixels: Pixels, size: (u32, u32), pin: HWND) {
        if self.save_as.is_some() && !has_room(&self.waiting) {
            self.notify(vec![Notice::SaveFailed(SaveError::Busy)], near);
            return;
        }
        self.ask_to_save(near, Origin::Pin, pixels, size, pin);
    }

    fn ask_to_save(
        &mut self,
        rect: Rect,
        origin: Origin,
        pixels: Pixels,
        (width, height): (u32, u32),
        previous: HWND,
    ) {
        let time = clock::local_time();
        let stem = self.file_name.stem(&time, width, height);
        let suggestion = Suggestion {
            name: file_name::png(&stem, 1),
            folder: None,
            usual_folder: self.save_folder.clone(),
            over: over(rect, origin),
        };
        let pending = PendingSaveAs {
            dialog: 0,
            rect,
            origin,
            unsaved: Unsaved::Pixels(pixels),
            time,
            previous,
        };
        self.ask_in_turn(pending, suggestion);
    }

    pub(super) fn ask_again(&mut self, job: u64, png: Png, path: PathBuf, time: LocalTime) {
        let (rect, origin) = self
            .origin_of(job)
            .unwrap_or((Rect::default(), Origin::Picture));
        let suggestion = asked_again(&path, rect, origin);
        let pending = PendingSaveAs {
            dialog: 0,
            rect,
            origin,
            unsaved: Unsaved::Encoded { job, png, path },
            time,
            previous: HWND::default(),
        };
        self.ask_in_turn(pending, suggestion);
    }

    pub(super) fn take_accepted_saves(&mut self) -> Vec<(u64, Png, PathBuf)> {
        accepted(self.save_as.take(), std::mem::take(&mut self.waiting))
    }

    fn ask_in_turn(&mut self, pending: PendingSaveAs, suggestion: Suggestion) {
        if self.save_as.is_some() {
            self.waiting.push_back(Waiting {
                pending,
                suggestion,
            });
            return;
        }
        let (rect, previous) = (pending.rect, pending.previous);
        let job = match &pending.unsaved {
            Unsaved::Pixels(_) => None,
            Unsaved::Encoded { job, .. } => Some(*job),
        };
        let Err(error) = self.ask(pending, suggestion) else {
            return;
        };
        match job {
            Some(job) => {
                let error = SaveError::Unexpected(error.to_string());
                self.update(job, |report| report.saved(Err(error)));
            }
            None => {
                foreground::give_back_from_us(previous);
                let error = DeliveryError::Unexpected(error.to_string());
                self.notify(vec![Notice::DeliveryFailed(error)], rect);
            }
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
            (Ok(Some(target)), Unsaved::Pixels(pixels)) => self.send_job(
                pending.rect,
                pending.origin,
                Ok(pixels),
                Delivery::SAVE,
                Some(target),
                pending.time,
            ),
            (Ok(Some(target)), Unsaved::Encoded { job, png, .. }) => {
                let save = Save {
                    job,
                    png,
                    target,
                    time: pending.time,
                };
                let queued = match &self.worker {
                    Some(worker) => worker.save(save),
                    None => Err(worker::save_stopped()),
                };
                if let Err(error) = queued {
                    self.update(job, |report| report.saved(Err(error)));
                }
            }
            (Ok(None), Unsaved::Pixels(_)) => {}
            (Ok(None), Unsaved::Encoded { job, .. }) => self.forget(job),
            (Err(error), Unsaved::Pixels(_)) => {
                let error = SaveError::Other(error.message());
                self.notify(vec![Notice::SaveFailed(error)], pending.rect);
            }
            (Err(error), Unsaved::Encoded { job, .. }) => {
                let error = SaveError::Other(error.message());
                self.update(job, |report| report.saved(Err(error)));
            }
        }
        while self.save_as.is_none() {
            let Some(next) = self.waiting.pop_front() else {
                break;
            };
            self.ask_in_turn(next.pending, next.suggestion);
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

// The dialog opens above the pin it saves, which would otherwise cover it.
fn over(rect: Rect, origin: Origin) -> Option<Rect> {
    (origin == Origin::Pin).then_some(rect)
}

fn asked_again(path: &Path, rect: Rect, origin: Origin) -> Suggestion {
    Suggestion {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        folder: path.parent().map(Path::to_path_buf),
        usual_folder: None,
        over: over(rect, origin),
    }
}

fn accepted(open: Option<PendingSaveAs>, waiting: VecDeque<Waiting>) -> Vec<(u64, Png, PathBuf)> {
    let waiting = waiting.into_iter().map(|waiting| waiting.pending);
    open.into_iter()
        .chain(waiting)
        .filter_map(|pending| match pending.unsaved {
            Unsaved::Encoded { job, png, path } => Some((job, png, path)),
            Unsaved::Pixels(_) => None,
        })
        .collect()
}

// Each new capture waits with all its pixels; saves already accepted are never turned away.
fn has_room(waiting: &VecDeque<Waiting>) -> bool {
    let images = waiting
        .iter()
        .filter(|waiting| matches!(waiting.pending.unsaved, Unsaved::Pixels(_)))
        .count();
    images < IMAGES_WAITING
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Arc;

    use bediz_core::file_name::LocalTime;
    use bediz_core::geometry::Rect;
    use bediz_core::image::Image;
    use windows::Win32::Foundation::HWND;

    use super::{
        IMAGES_WAITING, Origin, PendingSaveAs, Unsaved, Waiting, accepted, asked_again, has_room,
    };
    use crate::windows::save_dialog::Suggestion;
    use crate::windows::worker::Pixels;

    fn encoded(job: u64) -> Unsaved {
        let png = Arc::new(vec![0; 8]);
        let path = PathBuf::from(format!(r"C:\Users\Ada\Pictures\notes {job}.png"));
        Unsaved::Encoded { job, png, path }
    }

    fn image() -> Unsaved {
        Unsaved::Pixels(Pixels::Image(Arc::new(Image::blank(4, 4).unwrap())))
    }

    fn waiting(unsaved: Unsaved) -> Waiting {
        Waiting {
            pending: PendingSaveAs {
                dialog: 0,
                rect: Rect::default(),
                origin: Origin::Region,
                unsaved,
                time: LocalTime {
                    year: 2026,
                    month: 1,
                    day: 2,
                    hour: 3,
                    minute: 4,
                    second: 5,
                },
                previous: HWND::default(),
            },
            suggestion: Suggestion {
                name: "Bediz.png".to_owned(),
                folder: None,
                usual_folder: None,
                over: None,
            },
        }
    }

    #[test]
    fn new_captures_wait_in_a_bounded_line_and_accepted_saves_do_not_count() {
        let mut line = VecDeque::new();
        for job in 0..10 {
            line.push_back(waiting(encoded(job)));
        }
        for _ in 0..IMAGES_WAITING {
            assert!(has_room(&line));
            line.push_back(waiting(image()));
        }
        assert!(!has_room(&line));
    }

    #[test]
    fn quitting_keeps_every_accepted_save_and_drops_captures_never_named() {
        let open = Some(waiting(encoded(1)).pending);
        let line = VecDeque::from([waiting(image()), waiting(encoded(2)), waiting(encoded(3))]);
        let kept: Vec<(u64, PathBuf)> = accepted(open, line)
            .into_iter()
            .map(|(job, _, path)| (job, path))
            .collect();
        assert_eq!(
            kept,
            [
                (1, PathBuf::from(r"C:\Users\Ada\Pictures\notes 1.png")),
                (2, PathBuf::from(r"C:\Users\Ada\Pictures\notes 2.png")),
                (3, PathBuf::from(r"C:\Users\Ada\Pictures\notes 3.png")),
            ]
        );
        assert!(accepted(Some(waiting(image()).pending), VecDeque::new()).is_empty());
    }

    #[test]
    fn a_name_taken_meanwhile_is_asked_again_over_the_pin_it_saves() {
        let pin = Rect {
            left: 100,
            top: 50,
            right: 2660,
            bottom: 1490,
        };
        let path = PathBuf::from(r"C:\Users\Ada\Pictures\notes.png");
        let again = asked_again(&path, pin, Origin::Pin);
        assert_eq!(again.over, Some(pin));
        assert_eq!(again.name, "notes.png");
        assert_eq!(again.folder, Some(PathBuf::from(r"C:\Users\Ada\Pictures")));
        assert_eq!(again.usual_folder, None);
        assert_eq!(asked_again(&path, pin, Origin::Region).over, None);
        assert_eq!(asked_again(&path, pin, Origin::Picture).over, None);
    }
}
