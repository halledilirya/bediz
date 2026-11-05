use bediz_core::delivery::{CopyError, Delivery, DeliveryError, Report, SaveError};
use bediz_core::geometry::{Point, Rect};
use bediz_core::image::hex_colour;
use bediz_core::notice::Notice;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{CLIPBOARD_TIMER, Runtime};
use crate::event::Event;
use crate::windows::clipboard::{self, Contents, Placed};
use crate::windows::clock;
use crate::windows::session::Session;
use crate::windows::worker::{self, Job, Outcome, Worker};

const CLIPBOARD_RETRY_MILLISECONDS: u32 = 25;
const CLIPBOARD_ATTEMPTS: u32 = 10;

pub(super) struct PendingCopy {
    ticket: u64,
    contents: Contents,
    copied: Copied,
    attempts: u32,
}

enum Copied {
    Image { job: u64 },
    Colour { hex: String, near: Rect },
}

pub(super) struct Tracked {
    job: u64,
    near: Rect,
    report: Report,
}

impl Runtime {
    pub(super) fn deliver(&mut self, delivery: Delivery) {
        let Some(rect) = self.session.as_ref().and_then(Session::selected) else {
            return;
        };
        let read = self.scene.as_ref().map(|scene| scene.read(rect));
        self.hide();
        self.deliveries = self.deliveries.wrapping_add(1);
        let job = self.deliveries;
        self.reports.push(Tracked {
            job,
            near: rect,
            report: Report::new(delivery),
        });
        let image = match read {
            Some(Ok(image)) => image,
            Some(Err(error)) => {
                let error = DeliveryError::Other(error.to_string());
                self.update(job, |report| report.failed(error));
                return;
            }
            None => {
                let error = DeliveryError::Other("no display is ready".to_owned());
                self.update(job, |report| report.failed(error));
                return;
            }
        };
        let ticket = delivery.copy.then(|| self.copies.upcoming());
        let sent = match &self.worker {
            Some(worker) => worker.send(Job {
                id: job,
                image,
                copy: ticket,
                save: delivery.save,
                time: clock::local_time(),
            }),
            None => Err(worker::stopped()),
        };
        match (sent, ticket) {
            (Ok(()), Some(ticket)) => self.copies.accept(ticket),
            (Ok(()), None) => {}
            (Err(error), _) => self.update(job, |report| report.failed(error)),
        }
    }

    pub(super) fn copy_colour(&mut self, at: Point) {
        let colour = self.scene.as_ref().and_then(|scene| scene.pixel(at));
        self.hide();
        let near = Rect::new(at.x, at.y, at.x.saturating_add(1), at.y.saturating_add(1));
        match colour {
            Some(colour) => {
                let ticket = self.copies.upcoming();
                self.copies.accept(ticket);
                let hex = hex_colour(colour);
                let contents = Contents::text(&hex);
                self.place_on_clipboard(ticket, contents, Copied::Colour { hex, near });
            }
            None => {
                let error = CopyError::Other("no colour under the pointer".to_owned());
                self.notify(vec![Notice::CopyFailed(error)], near);
            }
        }
    }

    pub(super) fn on_outcomes(&mut self) {
        let outcomes = self
            .worker
            .as_ref()
            .map(Worker::take_outcomes)
            .unwrap_or_default();
        for outcome in outcomes {
            match outcome {
                Outcome::Encoded {
                    job,
                    ticket,
                    contents,
                } => self.place_on_clipboard(ticket, contents, Copied::Image { job }),
                Outcome::Saved { job, result } => self.save_settled(job, result),
                Outcome::Failed { job, error } => self.update(job, |report| report.failed(error)),
            }
        }
    }

    pub(super) fn retry_clipboard(&mut self) {
        let Some(mut pending) = self.clipboard.take() else {
            self.stop_clipboard_timer();
            return;
        };
        if !self.copies.is_latest(pending.ticket) {
            self.stop_clipboard_timer();
            self.supersede(pending.copied);
            return;
        }
        pending.attempts += 1;
        match clipboard::place(self.window.0, &pending.contents) {
            Ok(Placed::Done) => {
                self.stop_clipboard_timer();
                self.copy_settled(pending.copied, Ok(()));
            }
            Ok(Placed::Busy) => {
                let scheduled = pending.attempts < CLIPBOARD_ATTEMPTS
                    && unsafe {
                        SetTimer(
                            Some(self.window.0),
                            CLIPBOARD_TIMER,
                            CLIPBOARD_RETRY_MILLISECONDS,
                            None,
                        )
                    } != 0;
                if scheduled {
                    self.clipboard = Some(pending);
                } else {
                    self.stop_clipboard_timer();
                    self.copy_settled(pending.copied, Err(CopyError::Busy));
                }
            }
            Err(error) => {
                self.stop_clipboard_timer();
                self.copy_settled(pending.copied, Err(CopyError::Other(error.to_string())));
            }
        }
    }

    fn place_on_clipboard(&mut self, ticket: u64, contents: Contents, copied: Copied) {
        if !self.copies.is_latest(ticket) {
            self.supersede(copied);
            return;
        }
        if let Some(previous) = self.clipboard.take() {
            self.supersede(previous.copied);
        }
        self.clipboard = Some(PendingCopy {
            ticket,
            contents,
            copied,
            attempts: 0,
        });
        self.retry_clipboard();
    }

    fn copy_settled(&mut self, copied: Copied, result: Result<(), CopyError>) {
        match copied {
            Copied::Image { job } => {
                if result.is_ok() {
                    self.remember(job);
                }
                self.update(job, |report| report.copied(result));
            }
            Copied::Colour { hex, near } => {
                let notice = match result {
                    Ok(()) => Notice::ColourCopied(hex),
                    Err(error) => Notice::CopyFailed(error),
                };
                self.notify(vec![notice], near);
            }
        }
    }

    fn save_settled(&mut self, job: u64, result: Result<(), SaveError>) {
        if result.is_ok() {
            self.remember(job);
        }
        self.update(job, |report| report.saved(result));
    }

    fn supersede(&mut self, copied: Copied) {
        if let Copied::Image { job } = copied {
            self.update(job, Report::superseded);
        }
    }

    fn remember(&mut self, job: u64) {
        if let Some(tracked) = self.reports.iter().find(|tracked| tracked.job == job) {
            self.last_region = Some(tracked.near);
        }
    }

    fn update(&mut self, job: u64, change: impl FnOnce(&mut Report)) {
        let Some(index) = self.reports.iter().position(|tracked| tracked.job == job) else {
            return;
        };
        let complete = self.reports.get_mut(index).is_some_and(|tracked| {
            change(&mut tracked.report);
            tracked.report.is_complete()
        });
        if complete {
            let tracked = self.reports.swap_remove(index);
            self.notify(tracked.report.notices(), tracked.near);
        }
    }

    fn notify(&self, notices: Vec<Notice>, near: Rect) {
        if !notices.is_empty() {
            (self.sink)(Event::Notice { notices, near });
        }
    }

    fn stop_clipboard_timer(&self) {
        let _ = unsafe { KillTimer(Some(self.window.0), CLIPBOARD_TIMER) };
    }
}
