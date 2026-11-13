//! What the user is told after a capture, copy, save or recording, with each notice's locale id
//! and argument and whether it is worth showing a label for.

use thiserror::Error;

use crate::delivery::{CopyError, DeliveryError, SaveError};
use crate::recording::{RecordingStop, clock_text};

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum Notice {
    #[error("copied")]
    Copied,
    #[error("saved")]
    Saved,
    #[error("copied and saved")]
    CopiedAndSaved,
    #[error("copied colour")]
    ColourCopied(String),
    #[error("not copied: {0}")]
    CopyFailed(CopyError),
    #[error("not saved: {0}")]
    SaveFailed(SaveError),
    #[error("not copied or saved: {0}")]
    DeliveryFailed(DeliveryError),
    #[error("capture failed: {0}")]
    CaptureFailed(String),
    #[error("GIF saved")]
    GifSaved,
    #[error("GIF copied and saved")]
    GifCopiedAndSaved,
    #[error("GIF not deleted: {0}")]
    GifNotDeleted(String),
    #[error("recording stopped: {0}")]
    RecordingStopped(RecordingStop),
}

impl Notice {
    pub fn locale_id(&self) -> &'static str {
        match self {
            Self::Copied => "label-copied",
            Self::Saved => "label-saved",
            Self::CopiedAndSaved => "label-copied-and-saved",
            Self::ColourCopied(_) => "label-colour-copied",
            Self::CopyFailed(error) => error.locale_id(),
            Self::SaveFailed(error) => error.locale_id(),
            Self::DeliveryFailed(error) => error.locale_id(),
            Self::CaptureFailed(_) => "capture-error",
            Self::GifSaved => "label-gif-saved",
            Self::GifCopiedAndSaved => "label-gif-copied-and-saved",
            Self::GifNotDeleted(_) => "label-gif-not-deleted",
            Self::RecordingStopped(stop) => stop.locale_id(),
        }
    }

    // Other holds Windows' own text in the user's language; English details stay in the log.
    pub fn argument(&self) -> Option<(&'static str, String)> {
        match self {
            Self::ColourCopied(colour) => Some(("colour", colour.clone())),
            Self::CopyFailed(CopyError::Other(reason))
            | Self::SaveFailed(SaveError::Other(reason))
            | Self::DeliveryFailed(DeliveryError::Other(reason))
            | Self::GifNotDeleted(reason) => Some(("reason", reason.clone())),
            Self::RecordingStopped(RecordingStop::TimeLimit(limit)) => {
                Some(("limit", clock_text(*limit)))
            }
            _ => None,
        }
    }

    pub fn is_announcement(&self) -> bool {
        matches!(self, Self::RecordingStopped(_))
    }

    pub fn is_saved_gif(&self) -> bool {
        matches!(self, Self::GifSaved | Self::GifCopiedAndSaved)
    }

    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            Self::CopyFailed(_)
                | Self::SaveFailed(_)
                | Self::DeliveryFailed(_)
                | Self::CaptureFailed(_)
                | Self::GifNotDeleted(_)
        )
    }
}

pub fn worth_a_label(notices: &[Notice], confirmations: bool) -> bool {
    !notices.is_empty()
        && (confirmations
            || notices
                .iter()
                .any(|notice| notice.is_failure() || notice.is_announcement()))
}

// The GIF's own window already says it was saved; a label beside it would say it twice.
pub fn beside_gif_window(notices: Vec<Notice>, window_shown: bool) -> Vec<Notice> {
    if window_shown {
        notices
            .into_iter()
            .filter(|notice| !notice.is_saved_gif())
            .collect()
    } else {
        notices
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn failures_are_shown_even_without_confirmations() {
        let partly = [Notice::Copied, Notice::SaveFailed(SaveError::DiskFull)];
        assert!(worth_a_label(&partly, false));
        assert!(!worth_a_label(&[Notice::CopiedAndSaved], false));
        assert!(worth_a_label(&[Notice::CopiedAndSaved], true));
        assert!(!worth_a_label(&[], true));
    }

    #[test]
    fn a_recording_that_stopped_by_itself_is_always_announced() {
        let stopped = [
            Notice::RecordingStopped(RecordingStop::Memory),
            Notice::GifSaved,
        ];
        assert!(worth_a_label(&stopped, false));
        assert!(!stopped.iter().any(Notice::is_failure));
        assert!(!worth_a_label(&[Notice::GifSaved], false));
        assert!(worth_a_label(&[Notice::GifSaved], true));
    }

    #[test]
    fn the_gif_window_takes_the_place_of_a_saved_label_but_never_of_a_problem() {
        let saved = vec![
            Notice::RecordingStopped(RecordingStop::Memory),
            Notice::GifSaved,
            Notice::CopyFailed(CopyError::Busy),
        ];
        assert_eq!(
            beside_gif_window(saved.clone(), true),
            [
                Notice::RecordingStopped(RecordingStop::Memory),
                Notice::CopyFailed(CopyError::Busy),
            ]
        );
        assert_eq!(beside_gif_window(saved.clone(), false), saved);
        assert!(beside_gif_window(vec![Notice::GifCopiedAndSaved], true).is_empty());
        assert!(!worth_a_label(&[Notice::GifCopiedAndSaved], false));
        assert_eq!(
            Notice::GifCopiedAndSaved.locale_id(),
            "label-gif-copied-and-saved"
        );
    }

    #[test]
    fn a_time_limit_is_passed_as_minutes_and_seconds() {
        let notice = Notice::RecordingStopped(RecordingStop::TimeLimit(Duration::from_secs(90)));
        assert_eq!(notice.locale_id(), "recording-stopped-time-limit");
        assert_eq!(notice.argument(), Some(("limit", "1:30".to_owned())));
        assert_eq!(
            Notice::RecordingStopped(RecordingStop::Display).argument(),
            None
        );
    }

    #[test]
    fn the_colour_is_never_in_the_log_text() {
        let notice = Notice::ColourCopied("#FFA010".to_owned());
        assert!(!notice.to_string().contains("FFA010"));
        assert_eq!(notice.argument(), Some(("colour", "#FFA010".to_owned())));
    }

    #[test]
    fn reasons_are_passed_to_the_message() {
        let notice = Notice::SaveFailed(SaveError::Other("network gone".to_owned()));
        assert_eq!(notice.locale_id(), "save-error-other");
        assert_eq!(
            notice.argument(),
            Some(("reason", "network gone".to_owned()))
        );
        assert!(notice.is_failure());
        assert_eq!(Notice::Saved.argument(), None);
        assert!(!Notice::Saved.is_failure());
        let kept = Notice::GifNotDeleted("in use".to_owned());
        assert_eq!(kept.locale_id(), "label-gif-not-deleted");
        assert_eq!(kept.argument(), Some(("reason", "in use".to_owned())));
        assert!(kept.is_failure());
    }

    #[test]
    fn an_unexpected_detail_goes_to_the_log_but_not_to_the_label() {
        let detail = "the selection is on no monitor";
        for notice in [
            Notice::CopyFailed(CopyError::Unexpected(detail.to_owned())),
            Notice::SaveFailed(SaveError::Unexpected(detail.to_owned())),
            Notice::DeliveryFailed(DeliveryError::Unexpected(detail.to_owned())),
            Notice::CaptureFailed(detail.to_owned()),
        ] {
            assert_eq!(notice.argument(), None, "{notice}");
            assert!(notice.to_string().contains(detail), "{notice}");
            assert!(notice.is_failure());
        }
        assert_eq!(
            Notice::CopyFailed(CopyError::Unexpected(detail.to_owned())).locale_id(),
            "copy-error-unexpected"
        );
    }
}
