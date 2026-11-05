use thiserror::Error;

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
}
