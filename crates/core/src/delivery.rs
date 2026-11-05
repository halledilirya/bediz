use thiserror::Error;

use crate::notice::Notice;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub copy: bool,
    pub save: bool,
}

impl Delivery {
    pub const COPY_AND_SAVE: Self = Self {
        copy: true,
        save: true,
    };
    pub const COPY: Self = Self {
        copy: true,
        save: false,
    };
    pub const SAVE: Self = Self {
        copy: false,
        save: true,
    };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyOrder {
    latest: u64,
}

impl CopyOrder {
    pub fn upcoming(&self) -> u64 {
        self.latest.wrapping_add(1)
    }

    pub fn accept(&mut self, ticket: u64) {
        self.latest = ticket;
    }

    pub fn is_latest(&self, ticket: u64) -> bool {
        ticket == self.latest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CopyError {
    #[error("another app keeps the clipboard open")]
    Busy,
    #[error("{0}")]
    Other(String),
}

impl CopyError {
    pub fn locale_id(&self) -> &'static str {
        match self {
            Self::Busy => "copy-error-busy",
            Self::Other(_) => "copy-error-other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum SaveError {
    #[error("the disk is full")]
    DiskFull,
    #[error("access to the folder is denied")]
    AccessDenied,
    #[error("the folder does not exist")]
    FolderMissing,
    #[error("another app is using the file")]
    InUse,
    #[error("every file name for this second is taken")]
    NoFreeName,
    #[error("earlier screenshots are still being written")]
    Busy,
    #[error("{0}")]
    Other(String),
}

impl SaveError {
    pub fn locale_id(&self) -> &'static str {
        match self {
            Self::DiskFull => "save-error-disk-full",
            Self::AccessDenied => "save-error-access-denied",
            Self::FolderMissing => "save-error-folder-missing",
            Self::InUse => "save-error-in-use",
            Self::NoFreeName => "save-error-no-free-name",
            Self::Busy => "save-error-busy",
            Self::Other(_) => "save-error-other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum DeliveryError {
    #[error("earlier screenshots are still being encoded")]
    Busy,
    #[error("{0}")]
    Other(String),
}

impl DeliveryError {
    pub fn locale_id(&self) -> &'static str {
        match self {
            Self::Busy => "delivery-error-busy",
            Self::Other(_) => "delivery-error-other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part<E> {
    Skipped,
    Waiting,
    Done,
    Failed(E),
    Superseded,
}

impl<E> Part<E> {
    fn asked(asked: bool) -> Self {
        if asked { Self::Waiting } else { Self::Skipped }
    }

    fn settle(&mut self, result: Result<(), E>) {
        if matches!(self, Self::Waiting) {
            *self = match result {
                Ok(()) => Self::Done,
                Err(error) => Self::Failed(error),
            };
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    copy: Part<CopyError>,
    save: Part<SaveError>,
    failed: Option<DeliveryError>,
}

impl Report {
    pub fn new(delivery: Delivery) -> Self {
        Self {
            copy: Part::asked(delivery.copy),
            save: Part::asked(delivery.save),
            failed: None,
        }
    }

    pub fn copied(&mut self, result: Result<(), CopyError>) {
        self.copy.settle(result);
    }

    pub fn superseded(&mut self) {
        if matches!(self.copy, Part::Waiting) {
            self.copy = Part::Superseded;
        }
    }

    pub fn saved(&mut self, result: Result<(), SaveError>) {
        self.save.settle(result);
    }

    pub fn failed(&mut self, error: DeliveryError) {
        self.failed = Some(error);
    }

    pub fn is_complete(&self) -> bool {
        self.failed.is_some()
            || !(matches!(self.copy, Part::Waiting) || matches!(self.save, Part::Waiting))
    }

    pub fn notices(&self) -> Vec<Notice> {
        if let Some(error) = &self.failed {
            return vec![Notice::DeliveryFailed(error.clone())];
        }
        if matches!((&self.copy, &self.save), (Part::Done, Part::Done)) {
            return vec![Notice::CopiedAndSaved];
        }
        let copy = match &self.copy {
            Part::Done => Some(Notice::Copied),
            Part::Failed(error) => Some(Notice::CopyFailed(error.clone())),
            Part::Skipped | Part::Waiting | Part::Superseded => None,
        };
        let save = match &self.save {
            Part::Done => Some(Notice::Saved),
            Part::Failed(error) => Some(Notice::SaveFailed(error.clone())),
            Part::Skipped | Part::Waiting | Part::Superseded => None,
        };
        copy.into_iter().chain(save).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted(order: &mut CopyOrder) -> u64 {
        let ticket = order.upcoming();
        order.accept(ticket);
        ticket
    }

    #[test]
    fn only_the_latest_copy_reaches_the_clipboard() {
        let mut order = CopyOrder::default();
        let screenshot = accepted(&mut order);
        assert!(order.is_latest(screenshot));
        let colour = accepted(&mut order);
        assert!(!order.is_latest(screenshot));
        assert!(order.is_latest(colour));
    }

    #[test]
    fn a_refused_request_keeps_the_last_accepted_copy() {
        let mut order = CopyOrder::default();
        let accepted_copy = accepted(&mut order);
        let refused = order.upcoming();
        assert!(order.is_latest(accepted_copy));
        assert!(!order.is_latest(refused));
        assert_eq!(accepted(&mut order), refused);
        assert!(!order.is_latest(accepted_copy));
    }

    #[test]
    fn a_ticket_never_matches_before_it_is_accepted() {
        let order = CopyOrder::default();
        assert!(!order.is_latest(order.upcoming()));
    }

    #[test]
    fn a_report_waits_for_every_part_asked_for() {
        let mut report = Report::new(Delivery::COPY_AND_SAVE);
        assert!(!report.is_complete());
        report.copied(Ok(()));
        assert!(!report.is_complete());
        report.saved(Ok(()));
        assert!(report.is_complete());
        assert_eq!(report.notices(), [Notice::CopiedAndSaved]);
    }

    #[test]
    fn a_single_part_reports_alone() {
        let mut copy = Report::new(Delivery::COPY);
        copy.copied(Ok(()));
        assert!(copy.is_complete());
        assert_eq!(copy.notices(), [Notice::Copied]);
        let mut save = Report::new(Delivery::SAVE);
        save.saved(Ok(()));
        assert_eq!(save.notices(), [Notice::Saved]);
    }

    #[test]
    fn a_failed_part_is_reported_next_to_the_one_that_worked() {
        let mut report = Report::new(Delivery::COPY_AND_SAVE);
        report.copied(Ok(()));
        report.saved(Err(SaveError::DiskFull));
        assert_eq!(
            report.notices(),
            [Notice::Copied, Notice::SaveFailed(SaveError::DiskFull)]
        );
    }

    #[test]
    fn a_superseded_copy_says_nothing() {
        let mut report = Report::new(Delivery::COPY);
        report.superseded();
        assert!(report.is_complete());
        assert!(report.notices().is_empty());
        let mut both = Report::new(Delivery::COPY_AND_SAVE);
        both.superseded();
        both.saved(Ok(()));
        assert_eq!(both.notices(), [Notice::Saved]);
    }

    #[test]
    fn a_delivery_that_never_ran_reports_one_failure() {
        let mut report = Report::new(Delivery::COPY_AND_SAVE);
        report.failed(DeliveryError::Busy);
        assert!(report.is_complete());
        assert_eq!(
            report.notices(),
            [Notice::DeliveryFailed(DeliveryError::Busy)]
        );
    }

    #[test]
    fn a_part_settles_only_once() {
        let mut report = Report::new(Delivery::COPY);
        report.copied(Err(CopyError::Busy));
        report.copied(Ok(()));
        report.superseded();
        assert_eq!(report.notices(), [Notice::CopyFailed(CopyError::Busy)]);
    }
}
