use thiserror::Error;

use crate::delivery::{CopyError, DeliveryError, SaveError};

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
        }
    }

    pub fn argument(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::ColourCopied(colour) => Some(("colour", colour)),
            Self::CopyFailed(CopyError::Other(reason))
            | Self::SaveFailed(SaveError::Other(reason))
            | Self::DeliveryFailed(DeliveryError::Other(reason))
            | Self::CaptureFailed(reason) => Some(("reason", reason)),
            _ => None,
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            Self::CopyFailed(_)
                | Self::SaveFailed(_)
                | Self::DeliveryFailed(_)
                | Self::CaptureFailed(_)
        )
    }
}

pub fn worth_a_label(notices: &[Notice], confirmations: bool) -> bool {
    !notices.is_empty() && (confirmations || notices.iter().any(Notice::is_failure))
}

#[cfg(test)]
mod tests {
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
    fn the_colour_is_never_in_the_log_text() {
        let notice = Notice::ColourCopied("#FFA010".to_owned());
        assert!(!notice.to_string().contains("FFA010"));
        assert_eq!(notice.argument(), Some(("colour", "#FFA010")));
    }

    #[test]
    fn reasons_are_passed_to_the_message() {
        let notice = Notice::SaveFailed(SaveError::Other("network gone".to_owned()));
        assert_eq!(notice.locale_id(), "save-error-other");
        assert_eq!(notice.argument(), Some(("reason", "network gone")));
        assert!(notice.is_failure());
        assert_eq!(Notice::Saved.argument(), None);
        assert!(!Notice::Saved.is_failure());
    }
}
