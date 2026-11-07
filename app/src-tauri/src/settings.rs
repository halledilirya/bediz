use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use bediz_core::file_name;
use bediz_core::geometry::Rect;
use bediz_core::settings::{Parsed, Settings};
use bediz_platform::{Command, Platform};
use tauri::{AppHandle, Manager};

use crate::events::{Message, Messages};
use crate::settings_window;
use crate::startup::{AT_START, StartupStatus};
use crate::tray;
use crate::update;

const KEPT_NAME: &str = "settings.unreadable.json";
const KEPT_COPIES: u32 = 100;

type Persist = Box<dyn Fn(&Settings) -> io::Result<()> + Send + Sync>;
type Reverted = Box<dyn Fn(&Settings, &Settings, &str) + Send + Sync>;

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
    writing: bool,
    held: bool,
    stopping: bool,
}

impl Store {
    pub fn start(
        path: PathBuf,
        reverted: impl Fn(&Settings, &Settings, &str) + Send + Sync + 'static,
    ) -> io::Result<(Self, Option<String>)> {
        let read = fs::read_to_string(&path);
        let (settings, problem) = loaded(&read);
        let note = match (problem, &read) {
            (Some(problem), Ok(text)) => Some(match keep_copy(&path, text) {
                Ok(copy) => format!(
                    "{problem}; the file as it was is kept as {}",
                    copy.display()
                ),
                Err(error) => format!("{problem}; the file as it was could not be kept: {error}"),
            }),
            (problem, _) => problem,
        };
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
                writing: false,
                held: false,
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

    pub fn change(
        &self,
        edit: impl FnOnce(&Settings) -> Settings,
        apply: impl FnOnce(&Settings, &Settings),
    ) {
        let mut state = lock(&self.shared.state);
        while state.held {
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let settings = edit(&state.current);
        apply(&state.current, &settings);
        state.current = settings;
        state.revision += 1;
        state.pending = true;
        self.shared.changed.notify_all();
    }

    pub fn hold(&self, at_most: Duration) -> bool {
        let state = lock(&self.shared.state);
        let (mut state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, at_most, |state| {
                (state.pending || state.writing) && !state.stopping
            })
            .unwrap_or_else(PoisonError::into_inner);
        let written = !state.pending && !state.writing;
        state.held = written;
        written
    }

    pub fn resume(&self) {
        lock(&self.shared.state).held = false;
        self.shared.changed.notify_all();
    }

    pub fn shutdown(&self) {
        let writer = lock(&self.writer).take();
        if let Some(writer) = writer {
            lock(&self.shared.state).stopping = true;
            self.shared.changed.notify_all();
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
            state.writing = true;
            drop(state);
            let written = (self.persist)(&wanted);
            state = lock(&self.state);
            state.writing = false;
            self.changed.notify_all();
            match written {
                Ok(()) => state.saved = wanted,
                Err(error) => {
                    let saved = state.saved.clone();
                    let dropped = std::mem::replace(&mut state.current, saved);
                    state.revision += 1;
                    state.pending = false;
                    (self.reverted)(&dropped, &state.current, &error.to_string());
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn change(app: &AppHandle, edit: impl FnOnce(&Settings) -> Settings) {
    if let Some(store) = app.try_state::<Store>() {
        store.change(edit, |before, after| apply(app, before, after));
        update::settings_changed(app);
    }
}

// The window does not show the last region, so it is neither told nor sent the settings again.
pub fn keep_last_region(app: &AppHandle, region: Rect) {
    if let Some(store) = app.try_state::<Store>()
        && needs_keeping(&store.current(), region)
    {
        store.change(
            |current| Settings {
                last_region: Some(region),
                ..current.clone()
            },
            |_, _| {},
        );
    }
}

fn needs_keeping(current: &Settings, region: Rect) -> bool {
    current.last_region != Some(region)
}

pub fn apply(app: &AppHandle, before: &Settings, after: &Settings) {
    if let Some(platform) = app.try_state::<Platform>() {
        send(&platform, after);
        if before.start_with_windows != after.start_with_windows {
            let request = app
                .try_state::<StartupStatus>()
                .map_or(AT_START, |status| status.request());
            platform.send(Command::SetStartWithWindows {
                on: after.start_with_windows,
                request,
            });
        }
    }
    tray::show_start_with_windows(app, after.start_with_windows);
    if before.language != after.language
        && let Some(messages) = app.try_state::<Messages>()
    {
        let _ = messages.0.send(Message::Language);
    }
    if before.theme != after.theme {
        settings_window::show_theme(app, after.theme);
    }
}

pub fn send(platform: &Platform, settings: &Settings) {
    platform.send(Command::SetHotkey {
        hotkey: settings.hotkey,
        yields: settings.print_screen_yields,
    });
    platform.send(Command::SetAllocation(settings.allocation()));
    platform.send(Command::SetWindowDetection(settings.detect_windows));
    platform.send(Command::SetEnter(settings.enter.delivery()));
    platform.send(Command::SetSaving {
        folder: settings.save_folder.clone(),
        file_name: settings.file_name.clone(),
    });
    platform.send(Command::KeepOutOfHistory(settings.keep_out_of_history));
}

fn loaded(read: &io::Result<String>) -> (Settings, Option<String>) {
    let failure = match read.as_ref().map(|json| Settings::parse(json)) {
        Ok(Ok(Parsed { settings, rejected })) if rejected.is_empty() => return (settings, None),
        Ok(Ok(Parsed { settings, rejected })) => {
            let note = format!(
                "the settings {} could not be read; their defaults are used",
                rejected.join(", ")
            );
            return (settings, Some(note));
        }
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

// The next write replaces the file, so what this version could not read is kept beside it.
fn keep_copy(path: &Path, text: &str) -> io::Result<PathBuf> {
    for attempt in 1..=KEPT_COPIES {
        let copy = path.with_file_name(file_name::numbered(KEPT_NAME, attempt));
        match OpenOptions::new().write(true).create_new(true).open(&copy) {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                return Ok(copy);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if fs::read_to_string(&copy).is_ok_and(|kept| kept == text) {
                    return Ok(copy);
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every numbered name is taken",
    ))
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
        reverts: Receiver<(Settings, Settings, String)>,
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
        let reverted = move |dropped: &Settings, saved: &Settings, reason: &str| {
            let _ = reverts_sender.send((dropped.clone(), saved.clone(), reason.to_owned()));
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
        harness.store.change(
            |_| detecting(),
            |before, after| applied = Some((before.clone(), after.clone())),
        );
        assert_eq!(applied, Some((Settings::default(), detecting())));
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        assert_eq!(harness.store.current(), detecting());
        harness.answers.send(Ok(())).unwrap();
    }

    #[test]
    fn the_last_change_made_during_a_write_is_written_after_it() {
        let harness = harness();
        harness.store.change(|_| detecting(), |_, _| {});
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        harness.store.change(|_| Settings::default(), |_, _| {});
        harness.store.change(|_| labelling(), |_, _| {});
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
        harness.store.change(|_| detecting(), |_, _| {});
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
        harness.store.change(|_| detecting(), |_, _| {});
        harness.answers.send(Ok(())).unwrap();
        harness.store.shutdown();
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
    }

    #[test]
    fn holding_waits_for_the_last_change_then_keeps_new_ones_until_resumed() {
        let harness = harness();
        harness.store.change(|_| detecting(), |_, _| {});
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        harness.store.change(|_| labelling(), |_, _| {});
        let (held_sender, held) = mpsc::channel();
        let (changed_sender, changed) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let _ = held_sender.send(harness.store.hold(WAIT));
            });
            assert!(held.recv_timeout(BRIEFLY).is_err());
            harness.answers.send(Ok(())).unwrap();
            assert_eq!(harness.writes.recv_timeout(WAIT), Ok(labelling()));
            assert!(held.recv_timeout(BRIEFLY).is_err());
            harness.answers.send(Ok(())).unwrap();
            assert_eq!(held.recv_timeout(WAIT), Ok(true));
            scope.spawn(|| {
                harness.store.change(|_| Settings::default(), |_, _| {});
                let _ = changed_sender.send(());
            });
            assert!(changed.recv_timeout(BRIEFLY).is_err());
            assert_eq!(harness.store.current(), labelling());
            harness.store.resume();
            assert_eq!(changed.recv_timeout(WAIT), Ok(()));
        });
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(Settings::default()));
        harness.answers.send(Ok(())).unwrap();
    }

    #[test]
    fn holding_gives_up_on_a_write_that_does_not_end_and_holds_nothing() {
        let harness = harness();
        harness.store.change(|_| detecting(), |_, _| {});
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        assert!(!harness.store.hold(BRIEFLY));
        harness.store.change(|_| labelling(), |_, _| {});
        assert_eq!(harness.store.current(), labelling());
        harness.answers.send(Ok(())).unwrap();
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(labelling()));
        harness.answers.send(Ok(())).unwrap();
    }

    #[test]
    fn holding_with_nothing_to_write_holds_at_once() {
        let harness = harness();
        let started = std::time::Instant::now();
        assert!(harness.store.hold(WAIT));
        assert!(started.elapsed() < WAIT);
        harness.store.resume();
    }

    #[test]
    fn a_failed_write_brings_back_the_saved_settings_and_drops_later_changes() {
        let harness = harness();
        harness.store.change(|_| detecting(), |_, _| {});
        assert_eq!(harness.writes.recv_timeout(WAIT), Ok(detecting()));
        harness.store.change(|_| labelling(), |_, _| {});
        harness
            .answers
            .send(Err(io::Error::other("the disk is full")))
            .unwrap();
        let (dropped, reverted, reason) = harness.reverts.recv_timeout(WAIT).unwrap();
        assert_eq!(dropped, labelling());
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
        harness.store.change(|_| detecting(), |_, _| {});
        assert_eq!(harness.store.revised(), (detecting(), 1));
        harness.store.change(|_| detecting(), |_, _| {});
        assert_eq!(harness.store.revised().1, 2);
        harness.answers.send(Ok(())).unwrap();
    }

    #[test]
    fn a_change_is_made_to_the_current_settings() {
        let harness = harness();
        harness.store.change(|_| detecting(), |_, _| {});
        harness.store.change(
            |current| Settings {
                success_labels: true,
                ..current.clone()
            },
            |_, _| {},
        );
        assert_eq!(harness.store.current(), labelling());
        harness.answers.send(Ok(())).unwrap();
    }

    #[test]
    fn a_region_whose_write_failed_is_kept_again_when_it_comes_back() {
        let before = Rect::new(0, 0, 10, 10);
        let after = Rect::new(5, 5, 50, 50);
        let harness = harness();
        harness.store.change(
            |current| Settings {
                last_region: Some(before),
                ..current.clone()
            },
            |_, _| {},
        );
        harness.writes.recv_timeout(WAIT).unwrap();
        harness.answers.send(Ok(())).unwrap();
        assert!(needs_keeping(&harness.store.current(), after));
        harness.store.change(
            |current| Settings {
                last_region: Some(after),
                ..current.clone()
            },
            |_, _| {},
        );
        assert!(!needs_keeping(&harness.store.current(), after));
        harness.writes.recv_timeout(WAIT).unwrap();
        harness
            .answers
            .send(Err(io::Error::other("the disk is full")))
            .unwrap();
        harness.reverts.recv_timeout(WAIT).unwrap();
        assert_eq!(harness.store.current().last_region, Some(before));
        assert!(needs_keeping(&harness.store.current(), after));
    }

    #[test]
    fn a_first_start_uses_the_defaults_quietly() {
        let missing = Err(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(loaded(&missing), (Settings::default(), None));
    }

    #[test]
    fn a_saved_file_is_read() {
        let (settings, note) = loaded(&Ok(r#"{ "detectWindows": true }"#.to_owned()));
        assert!(settings.detect_windows);
        assert_eq!(note, None);
    }

    #[test]
    fn a_value_this_version_cannot_read_is_named_and_the_rest_is_kept() {
        let json = r#"{ "detectWindows": true, "enter": "copyAndUpload" }"#;
        let (settings, note) = loaded(&Ok(json.to_owned()));
        assert!(settings.detect_windows);
        assert_eq!(settings.enter, Settings::default().enter);
        assert!(
            note.as_deref()
                .is_some_and(|note| note.contains("enter") && note.ends_with("defaults are used")),
            "{note:?}"
        );
    }

    #[test]
    fn a_damaged_or_unreadable_file_falls_back_to_the_defaults_with_a_note() {
        let (settings, note) = loaded(&Ok("{ detectWindows".to_owned()));
        assert_eq!(settings, Settings::default());
        assert!(note.is_some_and(|note| note.ends_with("the defaults are used")));
        let denied = Err(io::Error::from(io::ErrorKind::PermissionDenied));
        let (settings, note) = loaded(&denied);
        assert_eq!(settings, Settings::default());
        assert!(note.is_some_and(|note| note.starts_with("the settings could not be read")));
    }
}
