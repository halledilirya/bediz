use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::capture::Allocation;
use crate::delivery::Delivery;
use crate::file_name::Pattern;
use crate::geometry::Rect;
use crate::hotkey::Hotkey;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub start_with_windows: bool,
    pub language: Option<String>,
    pub theme: Theme,
    pub accent: Accent,
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
    pub check_for_updates: bool,
    pub last_region: Option<Rect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Accent {
    #[default]
    Blue,
    Teal,
    Purple,
    Pink,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub settings: Settings,
    pub rejected: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_with_windows: true,
            language: None,
            theme: Theme::System,
            accent: Accent::Blue,
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
            check_for_updates: false,
            last_region: None,
        }
    }
}

impl Settings {
    // One value an older or newer version cannot read must not cost every other setting.
    pub fn parse(json: &str) -> Result<Parsed, InvalidSettings> {
        let invalid = |error: serde_json::Error| InvalidSettings(error.to_string());
        let Value::Object(mut fields) = serde_json::from_str(json).map_err(invalid)? else {
            return Err(InvalidSettings("the file holds no object".to_owned()));
        };
        let mut rejected = Vec::new();
        fields.retain(|name, value| {
            let alone = Map::from_iter([(name.clone(), value.clone())]);
            let readable = serde_json::from_value::<Self>(Value::Object(alone)).is_ok();
            if !readable {
                rejected.push(name.clone());
            }
            readable
        });
        let settings = serde_json::from_value(Value::Object(fields)).map_err(invalid)?;
        Ok(Parsed { settings, rejected })
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

    // Only what the window changed is taken, so a change made elsewhere meanwhile is kept.
    pub fn with_changes(&self, shown: &Self, wanted: &Self) -> Self {
        fn pick<T: Clone + PartialEq>(current: &T, shown: &T, wanted: &T) -> T {
            if wanted == shown {
                current.clone()
            } else {
                wanted.clone()
            }
        }
        Self {
            start_with_windows: pick(
                &self.start_with_windows,
                &shown.start_with_windows,
                &wanted.start_with_windows,
            ),
            language: pick(&self.language, &shown.language, &wanted.language),
            theme: pick(&self.theme, &shown.theme, &wanted.theme),
            accent: pick(&self.accent, &shown.accent, &wanted.accent),
            hotkey: pick(&self.hotkey, &shown.hotkey, &wanted.hotkey),
            ask_about_hotkey: pick(
                &self.ask_about_hotkey,
                &shown.ask_about_hotkey,
                &wanted.ask_about_hotkey,
            ),
            print_screen_yields: self.print_screen_yields,
            detect_windows: pick(
                &self.detect_windows,
                &shown.detect_windows,
                &wanted.detect_windows,
            ),
            success_labels: pick(
                &self.success_labels,
                &shown.success_labels,
                &wanted.success_labels,
            ),
            enter: pick(&self.enter, &shown.enter, &wanted.enter),
            save_folder: pick(&self.save_folder, &shown.save_folder, &wanted.save_folder),
            file_name: pick(&self.file_name, &shown.file_name, &wanted.file_name),
            keep_out_of_history: pick(
                &self.keep_out_of_history,
                &shown.keep_out_of_history,
                &wanted.keep_out_of_history,
            ),
            release_gpu_while_idle: pick(
                &self.release_gpu_while_idle,
                &shown.release_gpu_while_idle,
                &wanted.release_gpu_while_idle,
            ),
            check_for_updates: pick(
                &self.check_for_updates,
                &shown.check_for_updates,
                &wanted.check_for_updates,
            ),
            last_region: self.last_region,
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

    fn parse(json: &str) -> Settings {
        let parsed = Settings::parse(json).unwrap();
        assert_eq!(parsed.rejected, Vec::<String>::new(), "{json}");
        parsed.settings
    }

    #[test]
    fn the_defaults_start_with_windows_on_print_screen() {
        let settings = parse("{}");
        assert_eq!(settings, Settings::default());
        assert!(settings.start_with_windows);
        assert_eq!(settings.language, None);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.accent, Accent::Blue);
        assert_eq!(settings.last_region, None);
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
        assert!(!settings.check_for_updates);
    }

    #[test]
    fn missing_fields_keep_their_defaults() {
        let settings = parse(r#"{ "successLabels": true }"#);
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
        let settings = parse(r#"{ "detectWindows": true, "later": [1, 2] }"#);
        assert!(settings.detect_windows);
    }

    #[test]
    fn a_value_that_cannot_be_read_gets_its_default_and_keeps_the_others() {
        for (field, value) in [
            ("detectWindows", r#""yes""#),
            ("hotkey", r#""Q""#),
            ("fileName", r#""a/b""#),
            ("enter", r#""copyAndUpload""#),
            ("theme", r#""sepia""#),
            ("lastRegion", "[1, 2]"),
        ] {
            let json =
                format!(r#"{{ "successLabels": true, "{field}": {value}, "language": "tr" }}"#);
            let parsed = Settings::parse(&json).unwrap();
            assert_eq!(parsed.rejected, [field], "{json}");
            assert_eq!(
                parsed.settings,
                Settings {
                    success_labels: true,
                    language: Some("tr".to_owned()),
                    ..Settings::default()
                },
                "{json}"
            );
        }
    }

    #[test]
    fn every_value_that_cannot_be_read_is_named() {
        let parsed = Settings::parse(r#"{ "theme": 3, "accent": "teal", "enter": null }"#).unwrap();
        let mut rejected = parsed.rejected;
        rejected.sort();
        assert_eq!(rejected, ["enter", "theme"]);
        assert_eq!(parsed.settings.accent, Accent::Teal);
    }

    #[test]
    fn a_damaged_file_is_rejected() {
        assert!(Settings::parse(r#"{ "detectWindows": true"#).is_err());
        assert!(Settings::parse("").is_err());
        assert!(Settings::parse("null").is_err());
        assert!(Settings::parse("[]").is_err());
    }

    #[test]
    fn written_settings_read_back_the_same() {
        let settings = Settings {
            start_with_windows: false,
            language: Some("tr".to_owned()),
            theme: Theme::Dark,
            accent: Accent::Teal,
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
            check_for_updates: true,
            last_region: Some(Rect::new(-1920, 0, 640, 480)),
        };
        let json = settings.to_json().unwrap();
        assert!(json.contains(r#""theme": "dark""#), "{json}");
        assert!(json.contains(r#""left": -1920"#), "{json}");
        assert!(json.contains(r#""hotkey": "Ctrl+Shift+A""#), "{json}");
        assert!(json.contains(r#""enter": "copy""#), "{json}");
        assert!(json.contains(r#""releaseGpuWhileIdle": true"#), "{json}");
        assert!(json.contains(r#""checkForUpdates": true"#), "{json}");
        assert_eq!(parse(&json), settings);
    }

    #[test]
    fn enter_does_what_was_chosen() {
        assert_eq!(Enter::Copy.delivery(), Delivery::COPY);
        assert_eq!(Enter::Save.delivery(), Delivery::SAVE);
        assert_eq!(parse(r#"{ "enter": "save" }"#).enter, Enter::Save);
    }

    #[test]
    fn the_settings_window_never_changes_a_fact_it_does_not_show() {
        let region = Rect::new(10, 20, 30, 40);
        let current = Settings {
            print_screen_yields: true,
            last_region: Some(region),
            ..Settings::default()
        };
        let wanted = Settings {
            detect_windows: true,
            ..Settings::default()
        };
        let kept = current.with_changes(&Settings::default(), &wanted);
        assert!(kept.print_screen_yields);
        assert_eq!(kept.last_region, Some(region));
        assert!(kept.detect_windows);
    }

    #[test]
    fn a_change_made_elsewhere_while_the_window_changed_another_one_is_kept() {
        let shown = Settings::default();
        let current = Settings {
            start_with_windows: false,
            ..shown.clone()
        };
        let wanted = Settings {
            success_labels: true,
            ..shown.clone()
        };
        let kept = current.with_changes(&shown, &wanted);
        assert!(!kept.start_with_windows);
        assert!(kept.success_labels);
    }

    #[test]
    fn every_field_the_window_changed_is_taken() {
        let shown = Settings::default();
        let wanted = Settings {
            start_with_windows: false,
            language: Some("de".to_owned()),
            theme: Theme::Light,
            accent: Accent::Pink,
            hotkey: Hotkey {
                ctrl: true,
                alt: true,
                shift: false,
                key: Key::Letter('B'),
            },
            ask_about_hotkey: false,
            detect_windows: true,
            success_labels: true,
            enter: Enter::Save,
            save_folder: Some(PathBuf::from(r"E:\Shots")),
            file_name: Pattern::parse("Shot {date}").unwrap(),
            keep_out_of_history: true,
            release_gpu_while_idle: true,
            check_for_updates: true,
            ..shown.clone()
        };
        assert_eq!(Settings::default().with_changes(&shown, &wanted), wanted);
        assert_eq!(
            Settings::default().with_changes(&wanted, &wanted),
            Settings::default()
        );
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
