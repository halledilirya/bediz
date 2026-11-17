//! Puts images, colours, GIFs and typed text on the clipboard in the order they were asked for,
//! retrying while it is busy, and reports how each copy ended.

use std::path::Path;

use bediz_core::delivery::{CopyError, Report};
use bediz_core::geometry::{Point, Rect};
use bediz_core::image::hex_colour;
use bediz_core::notice::Notice;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{CLIPBOARD_TIMER, Runtime};
use crate::event::{Event, GifEvent};
use crate::windows::clipboard::{self, Contents, Placed};

pub(super) const CLIPBOARD_RETRY_MILLISECONDS: u32 = 25;
const CLIPBOARD_ATTEMPTS: u32 = 10;

pub(super) struct PendingCopy {
    ticket: u64,
    contents: Contents,
    copied: Copied,
    unchanged_since: Option<u32>,
    attempts: u32,
}

pub(super) enum Copied {
    Image {
        job: u64,
    },
    Colour {
        hex: String,
        near: Rect,
    },
    RecordedGif {
        gif: Option<u64>,
        notices: Vec<Notice>,
        near: Rect,
    },
    Gif {
        gif: u64,
    },
}

impl Runtime {
    pub(super) fn copy_colour(&mut self, at: Point) {
        let colour = self.scene.as_ref().and_then(|scene| scene.pixel(at));
        self.hide();
        let near = Rect::spanning(at, at);
        match colour {
            Some(colour) => {
                let ticket = self.copies.reserve();
                let hex = hex_colour(colour);
                let contents = Contents::text(&hex);
                self.place_on_clipboard(ticket, contents, Copied::Colour { hex, near }, None);
            }
            None => {
                let error = CopyError::Unexpected("no colour under the pointer".to_owned());
                self.notify(vec![Notice::CopyFailed(error)], near);
            }
        }
    }

    // A GIF can take minutes to write, and whatever was copied elsewhere meanwhile is not replaced.
    pub(super) fn copy_recorded_gif(
        &mut self,
        (ticket, unchanged_since): (u64, u32),
        gif: Option<u64>,
        path: &Path,
        notices: Vec<Notice>,
        near: Rect,
    ) {
        let copied = Copied::RecordedGif { gif, notices, near };
        self.place_on_clipboard(ticket, Contents::file(path), copied, Some(unchanged_since));
    }

    // The window's file check runs off this thread, so the copy takes its place when it is asked.
    pub(super) fn reserve_gif_copy(&mut self, gif: u64) {
        self.gif_copy = Some((gif, self.copies.reserve()));
    }

    pub(super) fn copy_gif(&mut self, gif: u64, path: &Path) {
        let Some((_, ticket)) = self.gif_copy.take_if(|(asked, _)| *asked == gif) else {
            return;
        };
        self.place_on_clipboard(ticket, Contents::file(path), Copied::Gif { gif }, None);
    }

    // A cut erases the text only once it is on the clipboard, so it is placed now or not at all.
    pub(super) fn place_text(&mut self, text: &str) -> windows::core::Result<Placed> {
        self.copies.reserve();
        if let Some(previous) = self.clipboard.take() {
            self.stop_clipboard_timer();
            self.supersede(previous.copied);
        }
        clipboard::place(self.window.0, &Contents::text(text), false, None)
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
        let placed = clipboard::place(
            self.window.0,
            &pending.contents,
            self.keep_out_of_history,
            pending.unchanged_since,
        );
        match placed {
            Ok(Placed::Done) => {
                self.stop_clipboard_timer();
                self.copy_settled(pending.copied, Ok(()));
            }
            Ok(Placed::Changed) => {
                self.stop_clipboard_timer();
                self.supersede(pending.copied);
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

    pub(super) fn place_on_clipboard(
        &mut self,
        ticket: u64,
        contents: Contents,
        copied: Copied,
        unchanged_since: Option<u32>,
    ) {
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
            unchanged_since,
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
            Copied::RecordedGif {
                gif,
                mut notices,
                near,
            } => {
                match &result {
                    Ok(()) => notices.push(Notice::GifCopiedAndSaved),
                    Err(error) => {
                        notices.push(Notice::GifSaved);
                        notices.push(Notice::CopyFailed(error.clone()));
                    }
                }
                if let Some(gif) = gif {
                    (self.sink)(Event::Gif(GifEvent::Copied { id: gif, result }));
                }
                self.notify(notices, near);
            }
            Copied::Gif { gif } => (self.sink)(Event::Gif(GifEvent::Copied { id: gif, result })),
        }
    }

    fn supersede(&mut self, copied: Copied) {
        match copied {
            Copied::Image { job } => self.update(job, Report::superseded),
            Copied::RecordedGif {
                mut notices, near, ..
            } => {
                notices.push(Notice::GifSaved);
                self.notify(notices, near);
            }
            Copied::Colour { .. } | Copied::Gif { .. } => {}
        }
    }

    fn stop_clipboard_timer(&self) {
        let _ = unsafe { KillTimer(Some(self.window.0), CLIPBOARD_TIMER) };
    }
}
