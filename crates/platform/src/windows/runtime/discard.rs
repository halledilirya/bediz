//! Deletes GIFs discarded after they were written, and reports any that could not be removed.

use std::ffi::c_void;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::{Runtime, WM_RECORDING};

type Removed = Arc<Mutex<Option<io::Result<()>>>>;

struct Deletion {
    near: Rect,
    removed: Removed,
    thread: JoinHandle<()>,
}

#[derive(Default)]
pub(super) struct Deletions(Vec<Deletion>);

impl Deletions {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn add(&mut self, near: Rect, removed: Removed, thread: JoinHandle<()>) {
        self.0.push(Deletion {
            near,
            removed,
            thread,
        });
    }

    fn finished(&mut self) -> Vec<(Rect, io::Error)> {
        let mut failures = Vec::new();
        for deletion in std::mem::take(&mut self.0) {
            let result = match answer(&deletion.removed) {
                Some(result) => result,
                // A thread that ended without answering is read once more, as its answer may have just come.
                None if deletion.thread.is_finished() => {
                    answer(&deletion.removed).unwrap_or_else(|| Err(stopped()))
                }
                None => {
                    self.0.push(deletion);
                    continue;
                }
            };
            let _ = deletion.thread.join();
            if let Err(error) = result {
                failures.push((deletion.near, error));
            }
        }
        failures
    }

    fn wait(&mut self) -> Vec<io::Error> {
        std::mem::take(&mut self.0)
            .into_iter()
            .filter_map(|deletion| {
                let _ = deletion.thread.join();
                answer(&deletion.removed)
                    .unwrap_or_else(|| Err(stopped()))
                    .err()
            })
            .collect()
    }
}

impl Runtime {
    // A share can take long to answer, so the file is removed off this thread and its answer awaited.
    pub(super) fn delete_discarded(&mut self, path: PathBuf, near: Rect) {
        let window = self.window.0.0 as isize;
        let removed = Removed::default();
        let answered = Arc::clone(&removed);
        let spawned = std::thread::Builder::new()
            .name("bediz-discard".to_owned())
            .spawn(move || {
                let result = gone(std::fs::remove_file(&path));
                *answered.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
                let window = HWND(window as *mut c_void);
                let _ = unsafe { PostMessageW(Some(window), WM_RECORDING, WPARAM(0), LPARAM(0)) };
            });
        match spawned {
            Ok(thread) => self.deleting.add(near, removed, thread),
            Err(error) => self.not_deleted(near, &error),
        }
    }

    pub(super) fn take_deletions(&mut self) {
        for (near, error) in self.deleting.finished() {
            self.not_deleted(near, &error);
        }
    }

    pub(super) fn finish_deletions(&mut self) {
        for error in self.deleting.wait() {
            self.emit_notes(vec![format!(
                "a GIF Bediz was told to delete is still there: {error}"
            )]);
        }
    }

    fn not_deleted(&mut self, near: Rect, error: &io::Error) {
        self.quit_failed();
        self.notify(vec![Notice::GifNotDeleted(error.to_string())], near);
    }
}

fn answer(removed: &Removed) -> Option<io::Result<()>> {
    removed
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

fn stopped() -> io::Error {
    io::Error::other("the deletion stopped before it answered")
}

fn gone(removed: io::Result<()>) -> io::Result<()> {
    match removed {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn near(left: i32) -> Rect {
        Rect::new(left, 0, left + 10, 10)
    }

    fn held() -> (JoinHandle<()>, mpsc::Sender<()>) {
        let (release, released) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = released.recv();
        });
        (thread, release)
    }

    fn in_use() -> io::Error {
        io::Error::from_raw_os_error(32)
    }

    #[test]
    fn every_deletion_is_followed_to_its_end_and_each_failure_reported() {
        let mut deletions = Deletions::default();
        let (first, release_first) = held();
        let (second, release_second) = held();
        let first_answer = Removed::default();
        let second_answer = Removed::default();
        deletions.add(near(0), Arc::clone(&first_answer), first);
        deletions.add(near(50), Arc::clone(&second_answer), second);
        *first_answer.lock().unwrap() = Some(Err(in_use()));
        release_first.send(()).unwrap();
        let failures = deletions.finished();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, near(0));
        assert!(!deletions.is_empty());
        assert!(deletions.finished().is_empty());
        *second_answer.lock().unwrap() = Some(Err(in_use()));
        release_second.send(()).unwrap();
        let failures = deletions.finished();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, near(50));
        assert_eq!(failures[0].1.raw_os_error(), Some(32));
        assert!(deletions.is_empty());
    }

    #[test]
    fn a_deletion_that_worked_leaves_nothing_to_report() {
        let mut deletions = Deletions::default();
        let (thread, release) = held();
        let answer = Removed::default();
        deletions.add(near(0), Arc::clone(&answer), thread);
        *answer.lock().unwrap() = Some(Ok(()));
        release.send(()).unwrap();
        assert!(deletions.finished().is_empty());
        assert!(deletions.is_empty());
    }

    #[test]
    fn a_thread_that_ended_without_an_answer_counts_as_a_failure() {
        let mut deletions = Deletions::default();
        let thread = std::thread::spawn(|| {});
        while !thread.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
        }
        deletions.add(near(0), Removed::default(), thread);
        assert_eq!(deletions.finished().len(), 1);
        assert!(deletions.is_empty());
    }

    #[test]
    fn on_the_way_out_every_deletion_is_waited_for() {
        let mut deletions = Deletions::default();
        let answer = Removed::default();
        let answered = Arc::clone(&answer);
        let thread = std::thread::spawn(move || {
            *answered.lock().unwrap() = Some(Err(in_use()));
        });
        deletions.add(near(0), answer, thread);
        let failures = deletions.wait();
        assert_eq!(failures.len(), 1);
        assert!(deletions.is_empty());
    }

    #[test]
    fn a_file_already_gone_counts_as_deleted_and_any_other_error_does_not() {
        let missing = io::Error::from(io::ErrorKind::NotFound);
        assert!(gone(Err(missing)).is_ok());
        assert!(gone(Ok(())).is_ok());
        assert_eq!(gone(Err(in_use())).unwrap_err().raw_os_error(), Some(32));
    }
}
