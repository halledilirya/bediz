use bediz_platform::{HotkeyState, Shortcut};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::settings_window;

const SETTINGS_NOT_SAVED: &str = "settings-not-saved";
const SETTINGS_CHANGED: &str = "settings-changed";
const HOTKEY_STATE: &str = "hotkey-state";
const GIF_HOTKEY_STATE: &str = "gif-hotkey-state";
const HOTKEY_PRESSED: &str = "hotkey-pressed";
const LANGUAGE_CHANGED: &str = "language-changed";
const START_WITH_WINDOWS_PROBLEM: &str = "start-with-windows-problem";
const UPDATE_STATUS: &str = "update-status";
const SHOW_SECTION: &str = "show-section";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdateStatus {
    NotChecked,
    Checking,
    UpToDate,
    Available {
        version: String,
    },
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    Installing {
        version: String,
    },
    CheckFailed {
        problem: UpdateProblem,
        reason: String,
    },
    InstallFailed {
        version: String,
        problem: UpdateProblem,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateProblem {
    Unreachable,
    NoRelease,
    Signature,
    Busy,
    SettingsUnsaved,
    Installer,
    Other,
}

impl UpdateStatus {
    pub const fn busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing { .. }
        )
    }

    pub const fn installable(&self) -> bool {
        matches!(self, Self::Available { .. } | Self::InstallFailed { .. })
    }

    pub fn available(&self) -> Option<&str> {
        match self {
            Self::Available { version }
            | Self::Downloading { version, .. }
            | Self::Installing { version }
            | Self::InstallFailed { version, .. } => Some(version),
            Self::NotChecked | Self::Checking | Self::UpToDate | Self::CheckFailed { .. } => None,
        }
    }
}

pub fn settings_not_saved(app: &AppHandle, reason: &str) {
    let _ = app.emit_to(settings_window::LABEL, SETTINGS_NOT_SAVED, reason);
}

pub fn settings_changed(app: &AppHandle) {
    let _ = app.emit_to(settings_window::LABEL, SETTINGS_CHANGED, ());
}

pub fn hotkey_state(app: &AppHandle, state: HotkeyState) {
    let _ = app.emit_to(settings_window::LABEL, HOTKEY_STATE, state_name(state));
}

pub fn gif_hotkey_state(app: &AppHandle, state: HotkeyState) {
    let _ = app.emit_to(settings_window::LABEL, GIF_HOTKEY_STATE, state_name(state));
}

pub fn language_changed(app: &AppHandle) {
    let _ = app.emit_to(settings_window::LABEL, LANGUAGE_CHANGED, ());
}

pub fn hotkey_pressed(app: &AppHandle, shortcut: Shortcut) {
    let _ = app.emit_to(
        settings_window::LABEL,
        HOTKEY_PRESSED,
        shortcut_name(shortcut),
    );
}

pub fn start_with_windows_problem(app: &AppHandle, problem: Option<&str>) {
    let _ = app.emit_to(settings_window::LABEL, START_WITH_WINDOWS_PROBLEM, problem);
}

pub fn update_status(app: &AppHandle, status: &UpdateStatus) {
    let _ = app.emit_to(settings_window::LABEL, UPDATE_STATUS, status);
}

pub fn show_section(app: &AppHandle) {
    let _ = app.emit_to(settings_window::LABEL, SHOW_SECTION, ());
}

pub const fn state_name(state: HotkeyState) -> &'static str {
    match state {
        HotkeyState::Registered => "registered",
        HotkeyState::SnippingTool => "snipping-tool",
        HotkeyState::Taken => "taken",
    }
}

pub const fn shortcut_name(shortcut: Shortcut) -> &'static str {
    match shortcut {
        Shortcut::Region => "region",
        Shortcut::Gif => "gif",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version() -> String {
        "0.2.0".to_owned()
    }

    fn check_failed() -> UpdateStatus {
        UpdateStatus::CheckFailed {
            problem: UpdateProblem::Unreachable,
            reason: "error sending request".to_owned(),
        }
    }

    fn install_failed() -> UpdateStatus {
        UpdateStatus::InstallFailed {
            version: version(),
            problem: UpdateProblem::Signature,
            reason: "invalid signature".to_owned(),
        }
    }

    #[test]
    fn a_check_or_an_install_never_starts_while_one_runs() {
        let busy = [
            UpdateStatus::Checking,
            UpdateStatus::Downloading {
                version: version(),
                percent: None,
            },
            UpdateStatus::Installing { version: version() },
        ];
        for status in &busy {
            assert!(status.busy(), "{status:?}");
            assert!(!status.installable(), "{status:?}");
        }
        let idle = [
            UpdateStatus::NotChecked,
            UpdateStatus::UpToDate,
            UpdateStatus::Available { version: version() },
            check_failed(),
            install_failed(),
        ];
        for status in &idle {
            assert!(!status.busy(), "{status:?}");
        }
    }

    #[test]
    fn only_an_announced_update_can_be_installed_and_tried_again() {
        assert!(UpdateStatus::Available { version: version() }.installable());
        assert!(install_failed().installable());
        assert!(!UpdateStatus::UpToDate.installable());
        assert!(!check_failed().installable());
    }

    #[test]
    fn the_tray_offers_the_version_until_it_is_gone() {
        let downloading = UpdateStatus::Downloading {
            version: version(),
            percent: Some(3),
        };
        assert_eq!(downloading.available(), Some("0.2.0"));
        assert_eq!(UpdateStatus::UpToDate.available(), None);
        assert_eq!(UpdateStatus::Checking.available(), None);
    }

    #[test]
    fn the_window_tells_the_shortcuts_apart_by_name() {
        assert_eq!(shortcut_name(Shortcut::Region), "region");
        assert_eq!(shortcut_name(Shortcut::Gif), "gif");
        assert_eq!(state_name(HotkeyState::SnippingTool), "snipping-tool");
    }

    #[test]
    fn the_window_reads_the_state_by_name() {
        let json = |status: UpdateStatus| serde_json::to_string(&status).unwrap();
        assert_eq!(json(UpdateStatus::NotChecked), r#"{"state":"not-checked"}"#);
        assert_eq!(
            json(UpdateStatus::Downloading {
                version: version(),
                percent: Some(40)
            }),
            r#"{"state":"downloading","version":"0.2.0","percent":40}"#
        );
        assert_eq!(
            json(install_failed()),
            r#"{"state":"install-failed","version":"0.2.0","problem":"signature","reason":"invalid signature"}"#
        );
        assert_eq!(
            json(check_failed()),
            r#"{"state":"check-failed","problem":"unreachable","reason":"error sending request"}"#
        );
    }
}
