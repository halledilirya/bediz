//! Readies the app for an update's installer: refused while anything is open or being written,
//! it ends every accepted job and copy first, and resumes work if the install fails.

use std::time::Duration;

use super::Runtime;
use super::delivery::CLIPBOARD_RETRY_MILLISECONDS;
use crate::windows::worker::Worker;

#[derive(Clone, Copy, Default)]
struct Open {
    selection: bool,
    save_as: bool,
    recording: bool,
    writing: bool,
    deleting: bool,
}

impl Open {
    fn any(self) -> bool {
        self.selection || self.save_as || self.recording || self.writing || self.deleting
    }
}

trait Barrier {
    fn busy(&self) -> bool;
    fn set_held(&mut self);
    fn take_held(&mut self) -> bool;
    fn finish_jobs(&mut self);
    fn copy_pending(&self) -> bool;
    fn retry_copy(&mut self);
    fn asking(&self) -> bool;
    fn has_worker(&self) -> bool;
    fn restart_worker(&mut self);
}

impl Runtime {
    pub(super) fn on_hold(&mut self) {
        let held = hold(self);
        let _ = self.saves_finished.send(held);
    }

    pub(super) fn resume(&mut self) {
        release(self);
    }
}

// An update ends the app right after starting its installer, so every accepted job ends first.
fn hold(barrier: &mut impl Barrier) -> bool {
    if barrier.busy() {
        return false;
    }
    barrier.set_held();
    barrier.finish_jobs();
    while barrier.copy_pending() {
        barrier.retry_copy();
    }
    if barrier.asking() {
        release(barrier);
        return false;
    }
    true
}

fn release(barrier: &mut impl Barrier) {
    if barrier.take_held() && !barrier.has_worker() {
        barrier.restart_worker();
    }
}

impl Barrier for Runtime {
    fn busy(&self) -> bool {
        Open {
            selection: self.session.is_some(),
            save_as: self.save_as.is_some(),
            recording: self.recording.is_some(),
            writing: !self.finishing.is_empty(),
            deleting: !self.deleting.is_empty(),
        }
        .any()
    }

    fn set_held(&mut self) {
        self.held = true;
    }

    fn take_held(&mut self) -> bool {
        std::mem::take(&mut self.held)
    }

    fn finish_jobs(&mut self) {
        let outcomes = self.worker.take().map(Worker::finish).unwrap_or_default();
        self.handle_outcomes(outcomes);
    }

    fn copy_pending(&self) -> bool {
        self.clipboard.is_some()
    }

    fn retry_copy(&mut self) {
        std::thread::sleep(Duration::from_millis(u64::from(
            CLIPBOARD_RETRY_MILLISECONDS,
        )));
        self.retry_clipboard();
    }

    fn asking(&self) -> bool {
        self.save_as.is_some()
    }

    fn has_worker(&self) -> bool {
        self.worker.is_some()
    }

    fn restart_worker(&mut self) {
        let mut notes = Vec::new();
        self.start_worker(&mut notes);
        self.emit_notes(notes);
    }
}

#[cfg(test)]
mod tests {
    use super::{Barrier, Open, hold, release};

    #[derive(Default)]
    struct Recorded {
        log: Vec<&'static str>,
        busy: bool,
        held: bool,
        worker: bool,
        copies_pending: u32,
        asks_after_finishing: bool,
        asking: bool,
    }

    impl Barrier for Recorded {
        fn busy(&self) -> bool {
            self.busy
        }

        fn set_held(&mut self) {
            self.log.push("held");
            self.held = true;
        }

        fn take_held(&mut self) -> bool {
            std::mem::take(&mut self.held)
        }

        fn finish_jobs(&mut self) {
            self.log.push("finished");
            self.worker = false;
            self.asking = self.asks_after_finishing;
        }

        fn copy_pending(&self) -> bool {
            self.copies_pending > 0
        }

        fn retry_copy(&mut self) {
            self.log.push("copied");
            self.copies_pending -= 1;
        }

        fn asking(&self) -> bool {
            self.asking
        }

        fn has_worker(&self) -> bool {
            self.worker
        }

        fn restart_worker(&mut self) {
            self.log.push("restarted");
            self.worker = true;
        }
    }

    fn running() -> Recorded {
        Recorded {
            worker: true,
            ..Recorded::default()
        }
    }

    #[test]
    fn an_open_selection_or_save_as_refuses_the_hold_before_anything_ends() {
        let mut barrier = Recorded {
            busy: true,
            ..running()
        };
        assert!(!hold(&mut barrier));
        assert!(barrier.log.is_empty());
        assert!(barrier.worker && !barrier.held);
    }

    #[test]
    fn the_hold_comes_only_once_every_job_and_pending_copy_has_ended() {
        let mut barrier = Recorded {
            copies_pending: 2,
            ..running()
        };
        assert!(hold(&mut barrier));
        assert_eq!(barrier.log, ["held", "finished", "copied", "copied"]);
        assert!(barrier.held && !barrier.worker);
    }

    #[test]
    fn a_save_as_opened_by_a_finished_job_frees_the_hold_again() {
        let mut barrier = Recorded {
            asks_after_finishing: true,
            ..running()
        };
        assert!(!hold(&mut barrier));
        assert_eq!(barrier.log, ["held", "finished", "restarted"]);
        assert!(!barrier.held && barrier.worker);
    }

    #[test]
    fn a_failed_install_resumes_with_a_new_worker_once() {
        let mut barrier = running();
        assert!(hold(&mut barrier));
        release(&mut barrier);
        release(&mut barrier);
        assert_eq!(barrier.log, ["held", "finished", "restarted"]);
        assert!(!barrier.held && barrier.worker);
    }

    #[test]
    fn a_gif_still_being_deleted_holds_the_update_back_like_one_being_written() {
        assert!(!Open::default().any());
        let deleting = Open {
            deleting: true,
            ..Open::default()
        };
        assert!(deleting.any());
        for open in [
            Open {
                selection: true,
                ..Open::default()
            },
            Open {
                save_as: true,
                ..Open::default()
            },
            Open {
                recording: true,
                ..Open::default()
            },
            Open {
                writing: true,
                ..Open::default()
            },
        ] {
            assert!(open.any());
        }
    }

    #[test]
    fn resuming_without_a_hold_changes_nothing() {
        let mut barrier = running();
        release(&mut barrier);
        assert!(barrier.log.is_empty());
    }
}
