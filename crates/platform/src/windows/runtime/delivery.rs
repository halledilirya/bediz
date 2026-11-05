use bediz_core::delivery::{CopyError, Delivery};
use bediz_core::geometry::{Point, Rect};
use bediz_core::image::hex_colour;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{CLIPBOARD_TIMER, Runtime};
use crate::event::{Event, Failure};
use crate::windows::clipboard::{self, Contents, Placed};
use crate::windows::clock;
use crate::windows::session::Session;
use crate::windows::worker::{Job, Outcome, Worker};

const CLIPBOARD_RETRY_MILLISECONDS: u32 = 25;
const CLIPBOARD_ATTEMPTS: u32 = 10;

pub(super) struct PendingCopy {
    ticket: u64,
    contents: Contents,
    copied: Copied,
    attempts: u32,
}

enum Copied {
    Image { rect: Rect, width: u32, height: u32 },
    Colour,
}

impl Runtime {
    pub(super) fn deliver(&mut self, delivery: Delivery) {
        let Some(rect) = self.session.as_ref().and_then(Session::selected) else {
            return;
        };
        let read = self.scene.as_ref().map(|scene| scene.read(rect));
        self.hide();
        let image = match read {
            Some(Ok(image)) => image,
            Some(Err(error)) => {
                self.fail(Failure::Capture(error.to_string()));
                return;
            }
            None => {
                self.fail(Failure::Displays("no display is ready".to_owned()));
                return;
            }
        };
        let ticket = delivery.copy.then(|| self.copies.upcoming());
        let job = Job {
            image,
            rect,
            copy: ticket,
            save: delivery.save,
            time: clock::local_time(),
        };
        let sent = match &self.worker {
            Some(worker) => worker.send(job),
            None => Err(Failure::Encode(
                "the threads that encode and save are not running".to_owned(),
            )),
        };
        match (sent, ticket) {
            (Ok(()), Some(ticket)) => self.copies.accept(ticket),
            (Ok(()), None) => {}
            (Err(failure), _) => self.fail(failure),
        }
    }

    pub(super) fn copy_colour(&mut self, at: Point) {
        let colour = self.scene.as_ref().and_then(|scene| scene.pixel(at));
        self.hide();
        match colour {
            Some(colour) => {
                let ticket = self.copies.upcoming();
                self.copies.accept(ticket);
                self.place_on_clipboard(
                    ticket,
                    Contents::text(&hex_colour(colour)),
                    Copied::Colour,
                );
            }
            None => self.fail(Failure::Copy(CopyError::Other(
                "no colour under the pointer".to_owned(),
            ))),
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
                    ticket,
                    rect,
                    width,
                    height,
                    contents,
                } => self.place_on_clipboard(
                    ticket,
                    contents,
                    Copied::Image {
                        rect,
                        width,
                        height,
                    },
                ),
                Outcome::Saved {
                    rect,
                    width,
                    height,
                } => {
                    self.last_region = Some(rect);
                    (self.sink)(Event::Saved { width, height });
                }
                Outcome::Failed(failure) => self.fail(failure),
            }
        }
    }

    pub(super) fn retry_clipboard(&mut self) {
        let Some(mut pending) = self
            .clipboard
            .take()
            .filter(|pending| self.copies.is_latest(pending.ticket))
        else {
            self.stop_clipboard_timer();
            return;
        };
        pending.attempts += 1;
        match clipboard::place(self.window.0, &pending.contents) {
            Ok(Placed::Done) => {
                self.stop_clipboard_timer();
                self.announce(pending.copied);
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
                    self.fail(Failure::Copy(CopyError::Busy));
                }
            }
            Err(error) => {
                self.stop_clipboard_timer();
                self.fail(Failure::Copy(CopyError::Other(error.to_string())));
            }
        }
    }

    fn place_on_clipboard(&mut self, ticket: u64, contents: Contents, copied: Copied) {
        if !self.copies.is_latest(ticket) {
            return;
        }
        self.clipboard = Some(PendingCopy {
            ticket,
            contents,
            copied,
            attempts: 0,
        });
        self.retry_clipboard();
    }

    fn stop_clipboard_timer(&self) {
        let _ = unsafe { KillTimer(Some(self.window.0), CLIPBOARD_TIMER) };
    }

    fn announce(&mut self, copied: Copied) {
        match copied {
            Copied::Image {
                rect,
                width,
                height,
            } => {
                self.last_region = Some(rect);
                (self.sink)(Event::Copied { width, height });
            }
            Copied::Colour => (self.sink)(Event::CopiedColour),
        }
    }

    fn fail(&self, failure: Failure) {
        (self.sink)(Event::Failed(failure));
    }
}
