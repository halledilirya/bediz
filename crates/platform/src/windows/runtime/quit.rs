use bediz_core::recording;

use super::Runtime;
use super::recording::Finishing;
use crate::event::{Event, QuitStep};

#[derive(Default)]
pub(super) struct Quitting {
    told: Option<Option<u8>>,
    waiting: bool,
    discarding: bool,
    failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Left {
    unsaved: bool,
    pending: bool,
    copying: bool,
    percent: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum News {
    Unsaved(Option<u8>),
    Ready,
    Failed,
}

impl Quitting {
    // A GIF that could not be saved or deleted stops the quit, so its error is seen before Bediz closes.
    fn news(&mut self, left: Left) -> Option<News> {
        if self.failed {
            return Some(News::Failed);
        }
        if left.unsaved {
            if self.told == Some(left.percent) {
                return None;
            }
            self.told = Some(left.percent);
            return Some(News::Unsaved(left.percent));
        }
        (!left.pending && !left.copying).then_some(News::Ready)
    }
}

impl Runtime {
    pub(super) fn on_quit(&mut self, step: QuitStep) {
        match step {
            QuitStep::Ask => {
                self.quit.get_or_insert_with(Quitting::default);
            }
            QuitStep::WhenSaved => {
                self.quit.get_or_insert_with(Quitting::default).waiting = true;
                self.stop_recording(None);
            }
            QuitStep::Discard => {
                self.quit.get_or_insert_with(Quitting::default).discarding = true;
                for finishing in &mut self.finishing {
                    finishing.discard();
                }
                self.cancel_recording();
            }
            QuitStep::Stay => self.quit = None,
        }
    }

    pub(super) fn closing(&self) -> bool {
        self.quitting
            || self
                .quit
                .as_ref()
                .is_some_and(|quit| quit.waiting || quit.discarding)
    }

    pub(super) fn quit_failed(&mut self) {
        if let Some(quit) = self.quit.as_mut() {
            quit.failed = true;
        }
    }

    // Read once each message is handled, never halfway through reporting the recordings that ended.
    pub(super) fn tell_quit(&mut self) {
        self.take_deletions();
        if self.quit.is_none() {
            return;
        }
        let left = Left {
            unsaved: self.recording.is_some()
                || self
                    .finishing
                    .iter()
                    .any(|finishing| finishing.written_share().is_some()),
            pending: self.recording.is_some()
                || !self.finishing.is_empty()
                || !self.deleting.is_empty(),
            copying: self.clipboard.is_some(),
            percent: recording::written_together(
                self.finishing.iter().filter_map(Finishing::written_share),
            ),
        };
        match self.quit.as_mut().and_then(|quit| quit.news(left)) {
            None => {}
            Some(News::Unsaved(percent)) => (self.sink)(Event::Unsaved(percent)),
            Some(News::Ready) => {
                self.quit = None;
                (self.sink)(Event::ReadyToQuit);
            }
            Some(News::Failed) => {
                self.quit = None;
                (self.sink)(Event::QuitCancelled);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left(unsaved: bool, pending: bool, copying: bool, percent: Option<u8>) -> Left {
        Left {
            unsaved,
            pending,
            copying,
            percent,
        }
    }

    #[test]
    fn the_share_written_is_told_once_each_time_it_changes() {
        let mut quit = Quitting::default();
        assert_eq!(
            quit.news(left(true, true, false, None)),
            Some(News::Unsaved(None))
        );
        assert_eq!(quit.news(left(true, true, false, None)), None);
        assert_eq!(
            quit.news(left(true, true, false, Some(40))),
            Some(News::Unsaved(Some(40)))
        );
        assert_eq!(quit.news(left(true, true, false, Some(40))), None);
    }

    #[test]
    fn a_saved_gif_still_going_onto_the_clipboard_holds_the_quit_until_it_is_there() {
        let mut quit = Quitting::default();
        quit.news(left(true, true, false, Some(99)));
        assert_eq!(quit.news(left(false, false, true, None)), None);
        assert_eq!(quit.news(left(false, false, true, None)), None);
        assert_eq!(
            quit.news(left(false, false, false, None)),
            Some(News::Ready)
        );
    }

    #[test]
    fn a_recording_whose_end_is_not_reported_yet_holds_the_quit() {
        let mut quit = Quitting {
            waiting: true,
            ..Quitting::default()
        };
        assert_eq!(quit.news(left(false, true, false, None)), None);
        quit.failed = true;
        assert_eq!(
            quit.news(left(false, false, false, None)),
            Some(News::Failed)
        );
    }

    #[test]
    fn nothing_left_lets_bediz_quit() {
        let mut quit = Quitting::default();
        assert_eq!(
            quit.news(left(false, false, false, None)),
            Some(News::Ready)
        );
    }

    #[test]
    fn a_gif_that_could_not_be_saved_or_deleted_stops_the_quit_instead_of_ending_it() {
        let mut quit = Quitting {
            discarding: true,
            failed: true,
            ..Quitting::default()
        };
        assert_eq!(
            quit.news(left(false, false, false, None)),
            Some(News::Failed)
        );
        assert_eq!(
            quit.news(left(true, true, false, Some(10))),
            Some(News::Failed)
        );
    }
}
