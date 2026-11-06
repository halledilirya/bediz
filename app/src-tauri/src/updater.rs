use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bediz_platform::Platform;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

use crate::events::{Message, Messages};
use crate::settings::Store;
use crate::update::{self, UpdateProblem, UpdateStatus};

const CHECK_AT_MOST: Duration = Duration::from_secs(30);
const DOWNLOAD_AT_MOST: Duration = Duration::from_secs(600);
const WRITE_AT_MOST: Duration = Duration::from_secs(10);

struct Inner {
    status: UpdateStatus,
    update: Option<Update>,
}

pub struct Updates(Mutex<Inner>);

impl Default for Updates {
    fn default() -> Self {
        Self(Mutex::new(Inner {
            status: UpdateStatus::NotChecked,
            update: None,
        }))
    }
}

impl Updates {
    pub fn status(&self) -> UpdateStatus {
        self.lock().status.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn check(app: &AppHandle) {
    let started = begin(app, |inner| {
        if inner.status.busy() {
            return None;
        }
        inner.status = UpdateStatus::Checking;
        inner.update = None;
        Some(())
    });
    if started.is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let found = match app.updater_builder().timeout(CHECK_AT_MOST).build() {
            Ok(updater) => updater.check().await,
            Err(error) => Err(error),
        };
        change(&app, |inner| match found {
            Ok(Some(update)) => {
                inner.status = UpdateStatus::Available {
                    version: update.version.clone(),
                };
                inner.update = Some(update);
            }
            Ok(None) => inner.status = UpdateStatus::UpToDate,
            Err(error) => {
                inner.status = UpdateStatus::CheckFailed {
                    problem: problem(&error),
                    reason: error.to_string(),
                };
            }
        });
    });
}

pub fn install(app: &AppHandle) {
    let started = begin(app, |inner| {
        if !inner.status.installable() {
            return None;
        }
        let mut update = inner.update.clone()?;
        update.timeout = Some(DOWNLOAD_AT_MOST);
        inner.status = UpdateStatus::Downloading {
            version: update.version.clone(),
            percent: Some(0),
        };
        Some(update)
    });
    let Some(update) = started else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let version = update.version.clone();
        let mut received = 0_u64;
        let mut shown = Some(0);
        let downloaded = update
            .download(
                |chunk, total| {
                    received = received.saturating_add(u64::try_from(chunk).unwrap_or(u64::MAX));
                    let percent = percent(received, total);
                    if percent != shown {
                        shown = percent;
                        change(&app, |inner| {
                            inner.status = UpdateStatus::Downloading {
                                version: version.clone(),
                                percent,
                            };
                        });
                    }
                },
                || {},
            )
            .await;
        let (problem, reason) = match downloaded {
            Ok(bytes) => {
                change(&app, |inner| {
                    inner.status = UpdateStatus::Installing {
                        version: version.clone(),
                    };
                });
                let ending = app.clone();
                let installed = tauri::async_runtime::spawn_blocking(move || {
                    let platform = ending.try_state::<Platform>();
                    let store = ending.try_state::<Store>();
                    install_held(
                        platform.as_deref().map(|platform| platform as &dyn Hold),
                        store.as_deref().map(|store| store as &dyn Hold),
                        || match catch_unwind(AssertUnwindSafe(|| update.install(&bytes))) {
                            Ok(Ok(())) => Ok(()),
                            Ok(Err(error)) => Err((problem(&error), error.to_string())),
                            Err(_) => Err((
                                UpdateProblem::Other,
                                "an internal error was caught while installing".to_owned(),
                            )),
                        },
                    )
                })
                .await;
                match installed {
                    Ok(Ok(())) => return,
                    Ok(Err(failure)) => failure,
                    Err(error) => (UpdateProblem::Other, error.to_string()),
                }
            }
            Err(error) => (problem(&error), error.to_string()),
        };
        change(&app, |inner| {
            inner.status = UpdateStatus::InstallFailed {
                version,
                problem,
                reason,
            };
        });
    });
}

type Failure = (UpdateProblem, String);

trait Hold {
    fn hold(&self) -> bool;
    fn resume(&self);
}

impl Hold for Platform {
    fn hold(&self) -> bool {
        Self::hold(self)
    }

    fn resume(&self) {
        Self::resume(self);
    }
}

impl Hold for Store {
    fn hold(&self) -> bool {
        Self::hold(self, WRITE_AT_MOST)
    }

    fn resume(&self) {
        Self::resume(self);
    }
}

// On Windows the plugin ends the app once the installer starts, so nothing may be in flight then.
fn install_held(
    platform: Option<&dyn Hold>,
    store: Option<&dyn Hold>,
    install: impl FnOnce() -> Result<(), Failure>,
) -> Result<(), Failure> {
    let resume = || {
        for held in [store, platform].into_iter().flatten() {
            held.resume();
        }
    };
    if platform.is_some_and(|platform| !platform.hold()) {
        return Err((
            UpdateProblem::Busy,
            "a screenshot or Save as was open".to_owned(),
        ));
    }
    if store.is_some_and(|store| !store.hold()) {
        resume();
        return Err((
            UpdateProblem::SettingsUnsaved,
            "the settings were still being written".to_owned(),
        ));
    }
    let installed = install();
    resume();
    installed
}

fn begin<T>(app: &AppHandle, edit: impl FnOnce(&mut Inner) -> Option<T>) -> Option<T> {
    let updates = app.try_state::<Updates>()?;
    let (started, status) = {
        let mut inner = updates.lock();
        let started = edit(&mut inner)?;
        (started, inner.status.clone())
    };
    update::update_status(app, &status);
    if let Some(messages) = app.try_state::<Messages>() {
        let _ = messages.0.send(Message::Updates(status));
    }
    Some(started)
}

fn change(app: &AppHandle, edit: impl FnOnce(&mut Inner)) {
    let _ = begin(app, |inner| {
        edit(inner);
        Some(())
    });
}

fn percent(received: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|total| *total > 0)?;
    u8::try_from((received.saturating_mul(100) / total).min(100)).ok()
}

fn problem(error: &Error) -> UpdateProblem {
    match error {
        Error::Reqwest(error) if error.is_decode() => UpdateProblem::NoRelease,
        Error::Reqwest(_) => UpdateProblem::Unreachable,
        Error::ReleaseNotFound
        | Error::TargetNotFound(_)
        | Error::TargetsNotFound(_)
        | Error::Serialization(_)
        | Error::Semver(_)
        | Error::Network(_) => UpdateProblem::NoRelease,
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::SignedVersionMismatch { .. }
        | Error::MissingSignedVersion => UpdateProblem::Signature,
        Error::Io(_) | Error::InvalidUpdaterFormat | Error::TempDirNotFound => {
            UpdateProblem::Installer
        }
        _ => UpdateProblem::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Recorded<'a> {
        name: &'static str,
        holds: bool,
        log: &'a RefCell<Vec<String>>,
    }

    impl Hold for Recorded<'_> {
        fn hold(&self) -> bool {
            self.log.borrow_mut().push(format!("hold {}", self.name));
            self.holds
        }

        fn resume(&self) {
            self.log.borrow_mut().push(format!("resume {}", self.name));
        }
    }

    fn install_with(
        platform_holds: bool,
        settings_hold: bool,
        installed: Result<(), Failure>,
    ) -> (Result<(), Failure>, Vec<String>) {
        let log = RefCell::new(Vec::new());
        let platform = Recorded {
            name: "platform",
            holds: platform_holds,
            log: &log,
        };
        let store = Recorded {
            name: "settings",
            holds: settings_hold,
            log: &log,
        };
        let result = install_held(Some(&platform), Some(&store), || {
            log.borrow_mut().push("install".to_owned());
            installed
        });
        (result, log.into_inner())
    }

    #[test]
    fn the_installer_starts_only_once_saves_and_settings_are_held_and_a_failure_frees_both() {
        let refused = Err((UpdateProblem::Installer, "access denied".to_owned()));
        let (result, log) = install_with(true, true, refused.clone());
        assert_eq!(result, refused);
        assert_eq!(
            log,
            [
                "hold platform",
                "hold settings",
                "install",
                "resume settings",
                "resume platform"
            ]
        );
    }

    #[test]
    fn an_open_screenshot_or_save_as_stops_the_install_before_anything_is_held() {
        let (result, log) = install_with(false, true, Ok(()));
        assert_eq!(
            result.map_err(|(problem, _)| problem),
            Err(UpdateProblem::Busy)
        );
        assert_eq!(log, ["hold platform"]);
    }

    #[test]
    fn settings_still_being_written_stop_the_install_and_free_the_platform() {
        let (result, log) = install_with(true, false, Ok(()));
        assert_eq!(
            result.map_err(|(problem, _)| problem),
            Err(UpdateProblem::SettingsUnsaved)
        );
        assert_eq!(
            log,
            [
                "hold platform",
                "hold settings",
                "resume settings",
                "resume platform"
            ]
        );
    }

    #[test]
    fn every_failure_is_told_by_its_kind() {
        assert_eq!(problem(&Error::ReleaseNotFound), UpdateProblem::NoRelease);
        assert_eq!(
            problem(&Error::Network(
                "Download request failed with status: 404".to_owned()
            )),
            UpdateProblem::NoRelease
        );
        assert_eq!(
            problem(&Error::TargetsNotFound(vec!["windows-x86_64".to_owned()])),
            UpdateProblem::NoRelease
        );
        assert_eq!(
            problem(&Error::SignedVersionMismatch {
                signed: "0.1.0".to_owned(),
                announced: "0.2.0".to_owned()
            }),
            UpdateProblem::Signature
        );
        assert_eq!(
            problem(&Error::MissingSignedVersion),
            UpdateProblem::Signature
        );
        assert_eq!(
            problem(&Error::Io(std::io::Error::other("access denied"))),
            UpdateProblem::Installer
        );
        assert_eq!(problem(&Error::EmptyEndpoints), UpdateProblem::Other);
    }

    #[test]
    fn the_download_shows_whole_percents_up_to_a_hundred() {
        assert_eq!(percent(0, Some(200)), Some(0));
        assert_eq!(percent(99, Some(200)), Some(49));
        assert_eq!(percent(200, Some(200)), Some(100));
        assert_eq!(percent(300, Some(200)), Some(100));
        assert_eq!(percent(u64::MAX, Some(1)), Some(100));
    }

    #[test]
    fn a_download_of_unknown_size_shows_no_percent() {
        assert_eq!(percent(100, None), None);
        assert_eq!(percent(100, Some(0)), None);
    }
}
