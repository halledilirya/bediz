//! Copies and saves a selection through the worker thread, keeps a report for each until both
//! have ended, and saves what was accepted before a quit.

use bediz_core::delivery::{Delivery, DeliveryError, Report, SaveError};
use bediz_core::file_name::LocalTime;
use bediz_core::geometry::Rect;
use bediz_core::image::Image;
use bediz_core::notice::Notice;

use super::Runtime;
use super::copies::Copied;
use crate::event::Event;
use crate::windows::clock;
use crate::windows::save;
use crate::windows::session::Purpose;
use crate::windows::worker::{self, Job, Outcome, Target, Worker};

pub(super) struct Tracked {
    job: u64,
    near: Rect,
    region: bool,
    report: Report,
}

impl Runtime {
    pub(super) fn deliver(&mut self, delivery: Delivery) {
        let Some((rect, read)) = self.read_selection() else {
            return;
        };
        let region = self.selects_region();
        let time = clock::local_time();
        let (width, height) = size_of(&read, rect);
        let target = delivery.save.then(|| Target::Folder {
            folder: self.save_folder.clone(),
            stem: self.file_name.stem(&time, width, height),
        });
        self.send_job(rect, region, read, delivery, target, time);
        self.hide();
    }

    // An opened picture is shown wherever it fits, so where it lay is no region to select again.
    pub(super) fn selects_region(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.purpose() != Purpose::Picture)
    }

    pub(super) fn forget(&mut self, job: u64) {
        self.reports.retain(|tracked| tracked.job != job);
    }

    pub(super) fn near_of(&self, job: u64) -> Option<Rect> {
        self.reports
            .iter()
            .find(|tracked| tracked.job == job)
            .map(|tracked| tracked.near)
    }

    pub(super) fn read_selection(&self) -> Option<(Rect, Result<Image, DeliveryError>)> {
        let session = self.session.as_ref()?;
        let rect = session.selected()?;
        let Some(scene) = self.scene.as_ref() else {
            let error = DeliveryError::Unexpected("no display is ready".to_owned());
            return Some((rect, Err(error)));
        };
        let read = match session.picture() {
            Some(picture) => session
                .marking()
                .compose(picture.image.clone(), picture.shown, scene),
            None => scene
                .read(rect)
                .and_then(|image| session.marking().compose(image, rect, scene)),
        };
        Some((
            rect,
            read.map_err(|error| DeliveryError::Unexpected(error.to_string())),
        ))
    }

    pub(super) fn send_job(
        &mut self,
        rect: Rect,
        region: bool,
        read: Result<Image, DeliveryError>,
        delivery: Delivery,
        save: Option<Target>,
        time: LocalTime,
    ) {
        self.deliveries = self.deliveries.wrapping_add(1);
        let job = self.deliveries;
        self.reports.push(Tracked {
            job,
            near: rect,
            region,
            report: Report::new(delivery),
        });
        let image = match read {
            Ok(image) => image,
            Err(error) => {
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
                save,
                time,
            }),
            None => Err(worker::stopped()),
        };
        match (sent, ticket) {
            (Ok(()), Some(ticket)) => self.copies.accept(ticket),
            (Ok(()), None) => {}
            (Err(error), _) => self.update(job, |report| report.failed(error)),
        }
    }

    pub(super) fn on_outcomes(&mut self) {
        let outcomes = self
            .worker
            .as_ref()
            .map(Worker::take_outcomes)
            .unwrap_or_default();
        self.handle_outcomes(outcomes);
    }

    pub(super) fn handle_outcomes(&mut self, outcomes: Vec<Outcome>) {
        for outcome in outcomes {
            match outcome {
                Outcome::Encoded {
                    job,
                    ticket,
                    contents,
                } => self.place_on_clipboard(ticket, contents, Copied::Image { job }, None),
                Outcome::Saved { job, result } => self.save_settled(job, result),
                Outcome::Appeared {
                    job,
                    png,
                    path,
                    time,
                } => self.ask_again(job, png, path, time),
                Outcome::Failed { job, error } => self.update(job, |report| report.failed(error)),
            }
        }
    }

    pub(super) fn start_worker(&mut self, notes: &mut Vec<String>) {
        match Worker::start(self.window.0) {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => notes.push(format!(
                "the thread that encodes and saves could not start: {error}"
            )),
        }
    }

    pub(super) fn on_finish_saves(&mut self) {
        self.finish_saves();
        let _ = self.saves_finished.send(true);
    }

    // On quit, copies not yet on the clipboard are dropped, but every accepted save still ends.
    pub(super) fn finish_saves(&mut self) {
        let outcomes = self.worker.take().map(Worker::finish).unwrap_or_default();
        let mut taken = Vec::new();
        for outcome in outcomes {
            match outcome {
                Outcome::Saved { job, result } => self.save_settled(job, result),
                Outcome::Appeared { job, png, path, .. } => taken.push((job, png, path)),
                Outcome::Encoded { .. } | Outcome::Failed { .. } => {}
            }
        }
        taken.extend(self.take_accepted_saves());
        for (job, png, path) in taken {
            let result = save::save_beside(&path, &png).map(drop);
            self.save_settled(job, result);
        }
        self.finish_recordings();
    }

    fn save_settled(&mut self, job: u64, result: Result<(), SaveError>) {
        if result.is_ok() {
            self.remember(job);
        }
        self.update(job, |report| report.saved(result));
    }

    pub(super) fn remember(&mut self, job: u64) {
        let Some(region) = self
            .reports
            .iter()
            .find(|tracked| tracked.job == job && tracked.region)
            .map(|tracked| tracked.near)
        else {
            return;
        };
        self.last_region = Some(region);
        (self.sink)(Event::LastRegion(region));
    }

    pub(super) fn update(&mut self, job: u64, change: impl FnOnce(&mut Report)) {
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

    pub(super) fn notify(&self, notices: Vec<Notice>, near: Rect) {
        self.tell(notices, near, false);
    }

    pub(super) fn notify_at_once(&self, notices: Vec<Notice>, near: Rect) {
        self.tell(notices, near, true);
    }

    fn tell(&self, notices: Vec<Notice>, near: Rect, at_once: bool) {
        if !notices.is_empty() {
            (self.sink)(Event::Notice {
                notices,
                near,
                at_once,
            });
        }
    }
}

fn size_of(read: &Result<Image, DeliveryError>, rect: Rect) -> (u32, u32) {
    match read {
        Ok(image) => (image.width(), image.height()),
        Err(_) => (rect.width(), rect.height()),
    }
}
