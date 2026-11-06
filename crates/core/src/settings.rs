use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::capture::Allocation;
use crate::delivery::Delivery;
use crate::file_name::Pattern;
use crate::hotkey::Hotkey;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub start_with_windows: bool,
    pub hotkey: Hotkey,
    pub ask_about_hotkey: bool,
    pub print_screen_yields: bool,
    pub detect_windows: bool,
    pub success_labels: bool,
    pub enter: Enter,
    pub save_folder: Option<PathBuf>,
    pub file_name: Pattern,
    pub keep_out_of_history: bool,
    pub release_gpu_while_idle: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Enter {
    #[default]
    CopyAndSave,
    Copy,
    Save,
}

#[derive(Debug, Error)]
#[error("the settings are not valid: {0}")]
pub struct InvalidSettings(String);

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_with_windows: true,
            hotkey: Hotkey::PRINT_SCREEN,
            ask_about_hotkey: true,
            print_screen_yields: false,
            detect_windows: false,
            success_labels: false,
            enter: Enter::CopyAndSave,
            save_folder: None,
            file_name: Pattern::default(),
            keep_out_of_history: false,
            release_gpu_while_idle: false,
        }
    }
}

impl Settings {
    pub fn parse(json: &str) -> Result<Self, InvalidSettings> {
        serde_json::from_str(json).map_err(|error| InvalidSettings(error.to_string()))
    }

    pub fn to_json(&self) -> Result<String, InvalidSettings> {
        serde_json::to_string_pretty(self).map_err(|error| InvalidSettings(error.to_string()))
    }

    pub const fn allocation(&self) -> Allocation {
        if self.release_gpu_while_idle {
            Allocation::OnHotkey
        } else {
            Allocation::Ahead
        }
    }

    pub fn keeping_facts_of(self, current: &Self) -> Self {
        Self {
            print_screen_yields: current.print_screen_yields,
            ..self
        }
    }
}

impl Enter {
    pub const fn delivery(self) -> Delivery {
        match self {
            Self::CopyAndSave => Delivery::COPY_AND_SAVE,
            Self::Copy => Delivery::COPY,
            Self::Save => Delivery::SAVE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::Key;

    #[test]
    fn the_defaults_start_with_windows_on_print_screen() {
        let settings = Settings::parse("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert!(settings.start_with_windows);
        assert_eq!(settings.hotkey, Hotkey::PRINT_SCREEN);
        assert!(settings.ask_about_hotkey);
        assert!(!settings.print_screen_yields);
        assert!(!settings.detect_windows);
        assert!(!settings.success_labels);
        assert_eq!(settings.enter.delivery(), Delivery::COPY_AND_SAVE);
        assert_eq!(settings.save_folder, None);
        assert_eq!(settings.file_name, Pattern::default());
        assert!(!settings.keep_out_of_history);
        assert_eq!(settings.allocation(), Allocation::Ahead);
    }

    #[test]
    fn missing_fields_keep_their_defaults() {
        let settings = Settings::parse(r#"{ "successLabels": true }"#).unwrap();
        assert_eq!(
            settings,
            Settings {
                success_labels: true,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn fields_from_a_newer_version_are_ignored() {
        let settings = Settings::parse(r#"{ "detectWindows": true, "later": [1, 2] }"#).unwrap();
        assert!(settings.detect_windows);
    }

    #[test]
    fn a_damaged_file_is_rejected() {
        assert!(Settings::parse(r#"{ "detectWindows": "yes" }"#).is_err());
        assert!(Settings::parse(r#"{ "detectWindows": true"#).is_err());
        assert!(Settings::parse(r#"{ "hotkey": "Q" }"#).is_err());
        assert!(Settings::parse(r#"{ "fileName": "a/b" }"#).is_err());
        assert!(Settings::parse(r#"{ "enter": "print" }"#).is_err());
        assert!(Settings::parse("").is_err());
        assert!(Settings::parse("null").is_err());
    }

    #[test]
    fn written_settings_read_back_the_same() {
        let settings = Settings {
            start_with_windows: false,
            hotkey: Hotkey {
                ctrl: true,
                alt: false,
                shift: true,
                key: Key::Letter('A'),
            },
            ask_about_hotkey: false,
            print_screen_yields: true,
            detect_windows: true,
            success_labels: false,
            enter: Enter::Copy,
            save_folder: Some(PathBuf::from(r"D:\Captures")),
            file_name: Pattern::parse("Shot {width}x{height}").unwrap(),
            keep_out_of_history: true,
            release_gpu_while_idle: true,
        };
        let json = settings.to_json().unwrap();
        assert!(json.contains(r#""hotkey": "Ctrl+Shift+A""#), "{json}");
        assert!(json.contains(r#""enter": "copy""#), "{json}");
        assert!(json.contains(r#""releaseGpuWhileIdle": true"#), "{json}");
        assert_eq!(Settings::parse(&json).unwrap(), settings);
    }

    #[test]
    fn enter_does_what_was_chosen() {
        assert_eq!(Enter::Copy.delivery(), Delivery::COPY);
        assert_eq!(Enter::Save.delivery(), Delivery::SAVE);
        assert_eq!(
            Settings::parse(r#"{ "enter": "save" }"#).unwrap().enter,
            Enter::Save
        );
    }

    #[test]
    fn the_settings_window_never_changes_a_fact_it_does_not_show() {
        let current = Settings {
            print_screen_yields: true,
            ..Settings::default()
        };
        let wanted = Settings {
            detect_windows: true,
            ..Settings::default()
        };
        let kept = wanted.keeping_facts_of(&current);
        assert!(kept.print_screen_yields);
        assert!(kept.detect_windows);
    }

    #[test]
    fn releasing_gpu_memory_allocates_on_the_hotkey() {
        let settings = Settings {
            release_gpu_while_idle: true,
            ..Settings::default()
        };
        assert_eq!(settings.allocation(), Allocation::OnHotkey);
    }
}
