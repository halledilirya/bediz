use std::sync::{Mutex, MutexGuard, PoisonError};

pub const AT_START: u64 = 0;

#[derive(Default)]
pub struct StartupStatus(Mutex<Status>);

#[derive(Default)]
struct Status {
    latest: u64,
    problem: Option<String>,
}

impl StartupStatus {
    pub fn request(&self) -> u64 {
        let mut status = self.lock();
        status.latest = status.latest.wrapping_add(1);
        status.latest
    }

    pub fn answered(&self, request: u64, failure: Option<String>) -> bool {
        let mut status = self.lock();
        if request != status.latest {
            return false;
        }
        status.problem = failure;
        true
    }

    pub fn problem(&self) -> Option<String> {
        self.lock().problem.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Status> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied() -> Option<String> {
        Some("access is denied".to_owned())
    }

    #[test]
    fn a_failure_at_start_is_kept_to_be_shown() {
        let status = StartupStatus::default();
        assert!(status.answered(AT_START, denied()));
        assert_eq!(status.problem(), denied());
    }

    #[test]
    fn two_failed_changes_leave_the_problem_of_the_last_one() {
        let status = StartupStatus::default();
        let off = status.request();
        let on = status.request();
        assert!(!status.answered(off, denied()));
        assert_eq!(status.problem(), None);
        assert!(status.answered(on, denied()));
        assert_eq!(status.problem(), denied());
    }

    #[test]
    fn a_later_success_clears_the_problem() {
        let status = StartupStatus::default();
        assert!(status.answered(AT_START, denied()));
        let again = status.request();
        assert!(status.answered(again, None));
        assert_eq!(status.problem(), None);
    }

    #[test]
    fn an_older_answer_never_replaces_a_newer_one() {
        let status = StartupStatus::default();
        let first = status.request();
        let second = status.request();
        assert!(status.answered(second, None));
        assert!(!status.answered(first, denied()));
        assert!(!status.answered(AT_START, denied()));
        assert_eq!(status.problem(), None);
    }
}
