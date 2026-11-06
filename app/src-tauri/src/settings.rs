use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use bediz_core::settings::Settings;
use bediz_platform::{Command, Platform};

type Persist = Box<dyn Fn(&Settings) -> io::Result<()> + Send + Sync>;
type Reverted = Box<dyn Fn(&Settings, &str) + Send + Sync>;

pub struct Store {
    shared: Arc<Shared>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    persist: Persist,
    reverted: Reverted,
}

struct State {
    current: Settings,
    revision: u64,
    saved: Settings,
    pending: bool,
    stopping: bool,
}

impl Store {
    pub fn start(
        path: PathBuf,
        reverted: impl Fn(&Settings, &str) + Send + Sync + 'static,
    ) -> io::Result<(Self, Option<String>)> {
        let (settings, note) = loaded(fs::read_to_string(&path));
        let persist = move |settings: &Settings| {
            let json = settings.to_json().map_err(io::Error::other)?;
            write(&path, &json)
        };
        let store = Self::with(settings, Box::new(persist), Box::new(reverted))?;
        Ok((store, note))
    }

    fn with(settings: Settings, persist: Persist, reverted: Reverted) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                current: settings.clone(),
                revision: 0,
                saved: settings,
                pending: false,
                stopping: false,
            }),
            changed: Condvar::new(),
            persist,
            reverted,
        });
        let writing = Arc::clone(&shared);
        let writer = std::thread::Builder::new()
            .name("bediz-settings".to_owned())
            .spawn(move || writing.write_changes())?;
        Ok(Self {
            shared,
            writer: Mutex::new(Some(writer)),
        })
    }

    pub fn current(&self) -> Settings {
        lock(&self.shared.state).current.clone()
    }

    pub fn revised(&self) -> (Settings, u64) {
        let state = lock(&self.shared.state);
        (state.current.clone(), state.revision)
    }

    pub fn change(&self, settings: Settings, apply: impl FnOnce(&Settings)) {
        let mut state = lock(&self.shared.state);
        apply(&settings);
        state.current = settings;
        state.revision += 1;
        state.pending = true;
        self.shared.changed.notify_one();
    }

    pub fn shutdown(&self) {
        let writer = lock(&self.writer).take();
        if let Some(writer) = writer {
            lock(&self.shared.state).stopping = true;
            self.shared.changed.notify_one();
            let _ = writer.join();
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Shared {
    fn write_changes(&self) {
        let mut state = lock(&self.state);
        loop {
            while !state.pending && !state.stopping {
                state = self
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if !state.pending {
                return;
            }
            let wanted = state.current.clone();
            state.pending = false;
            drop(state);
            let written = (self.persist)(&wanted);
            state = lock(&self.state);
            match written {
                Ok(()) => state.saved = wanted,
                Err(error) => {
                    state.current = state.saved.clone();
                    state.revision += 1;
                    state.pending = false;
                    (self.reverted)(&state.current, &error.to_string());
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn apply(platform: &Platform, settings: &Settings) {
    platform.send(Command::SetAllocation(settings.allocation()));
    platform.send(Command::SetWindowDetection(settings.detect_windows));
}

fn loaded(read: io::Result<String>) -> (Settings, Option<String>) {
    let failure = match read.map(|json| Settings::parse(&json)) {
        Ok(Ok(settings)) => return (settings, None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (Settings::default(), None);
        }
        Ok(Err(error)) => error.to_string(),
        Err(error) => format!("the settings could not be read: {error}"),
    };
    (
        Settings::default(),
        Some(format!("{failure}; the defaults are used")),
    )
}

fn write(path: &Path, json: &str) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let temporary = path.with_extension("json.tmp");
    let written = File::create(&temporary).and_then(|mut file| {
        file.write_all(json.as_bytes())?;
        file.sync_all()
    });
    // Replacing is intended: this is the app's own file; the no-overwrite rule is for captures.
    let result = written.and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::Duration;

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);
    const BRIEFLY: Duration = Duration::from_millis(100);

    struct Harness {
        answers: Sender<io::Result<()>>,
        store: Store,
        writes: Receiver<Settings>,
        reverts: Receiver<(Settings, String)>,
    }

    fn harness() -> Harness {
        let (writes_sender, writes) = mpsc::channel();
        let (answers, answers_receiver) = mpsc::channel();
        let (reverts_sender, reverts) = mpsc::channel();
        let answers_receiver = Mutex::new(answers_receiver);
        let persist = move |settings: &Settings| {
            let _ = writes_sender.send(settings.clone());
            lock(&answers_receiver).recv().unwrap_or(Ok(()))
        };
        let reverted = move |settings: &Settings, reason: &str| {
            let _ = reverts_sender.send((settings.clone(), reason.to_owned()));
        };
        let store =
            Store::with(Settings::default(), Box::new(persist), Box::new(reverted)).unwrap();
        Harness {
            answers,
            store,
            writes,
            reverts,
        }
    }

    fn detecting() -> Settings {
        Settings {
            detect_windows: true,
            ..Settings::default()
        }
    }

    fn labelling() -> Settings {
        Settings {
            success_labels: true,
            ..detecting()
        }
    }

    #[test]
    fn a_change_applies_at_once_and_reads_back_while_it_is_written() {
        let harness = harness();
        let mut applied = None;
        harness
            .store
            .change(detecting(), |settings| applied = Some(settings.clone()));
        assert_eq!(applied, Some(detecting()));
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        assert_eq!(harness.store.current(), detecting());
        harness.answers.send(Ok(())).unwrap();
    }

    #[test]
    fn the_last_change_made_during_a_write_is_written_after_it() {
        let harness = harness();
        harness.store.change(detecting(), |_| {});
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        harness.store.change(Settings::default(), |_| {});
        harness.store.change(labelling(), |_| {});
        harness.answers.send(Ok(())).unwrap();
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(labelling()));
        harness.answers.send(Ok(())).unwrap();
        harness.store.shutdown();
        assert!(harness.writes.try_recv().is_err());
        assert!(harness.reverts.try_recv().is_err());
    }

    #[test]
    fn shutting_down_waits_for_an_accepted_change() {
        let harness = harness();
        harness.store.change(detecting(), |_| {});
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        let (done_sender, done) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                harness.store.shutdown();
                let _ = done_sender.send(());
            });
            assert!(done.recv_timeout(BRIEFLY).is_err());
            harness.answers.send(Ok(())).unwrap();
            assert_eq!(done.recv_timeout(WAIT), Ok(()));
        });
    }

    #[test]
    fn a_change_accepted_just_before_shutting_down_is_still_written() {
        let harness = harness();
        harness.store.change(detecting(), |_| {});
        harness.answers.send(Ok(())).unwrap();
        harness.store.shutdown();
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
    }

    #[test]
    fn a_failed_write_brings_back_the_saved_settings_and_drops_later_changes() {
        let harness = harness();
        harness.store.change(detecting(), |_| {});
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        harness.store.change(labelling(), |_| {});
        harness
            .answers
            .send(Err(io::Error::other("the disk is full")))
            .unwrap();
        let (reverted, reason) = harness.reverts.recv_timeout(WAIT).unwrap();
        assert_eq!(reverted, Settings::default());
        assert!(reason.contains("the disk is full"), "{reason}");
        assert_eq!(harness.store.revised(), (Settings::default(), 3));
        harness.store.shutdown();
        assert!(harness.writes.try_recv().is_err());
    }

    #[test]
    fn every_change_moves_the_revision_on() {
        let harness = harness();
        assert_eq!(harness.store.revised(), (Settings::default(), 0));
        harness.store.change(detecting(), |_| {});
        assert_eq!(harness.store.revised(), (detecting(), 1));
        harness.store.change(detecting(), |_| {});
        assert_eq!(harness.store.revised().1, 2);
        harness.answers.send(Ok(())).unwrap();
    }

    #[test]
    fn a_first_start_uses_the_defaults_quietly() {
        let missing = Err(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(loaded(missing), (Settings::default(), None));
    }

    #[test]
    fn a_saved_file_is_read() {
        let (settings, note) = loaded(Ok(r#"{ "detectWindows": true }"#.to_owned()));
        assert!(settings.detect_windows);
        assert_eq!(note, None);
    }

    #[test]
    fn a_damaged_or_unreadable_file_falls_back_to_the_defaults_with_a_note() {
        let (settings, note) = loaded(Ok("{ detectWindows".to_owned()));
        assert_eq!(settings, Settings::default());
        assert!(note.is_some_and(|note| note.ends_with("the defaults are used")));
        let denied = Err(io::Error::from(io::ErrorKind::PermissionDenied));
        let (settings, note) = loaded(denied);
        assert_eq!(settings, Settings::default());
        assert!(note.is_some_and(|note| note.starts_with("the settings could not be read")));
    }
}
