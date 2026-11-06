use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::capture::Allocation;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub detect_windows: bool,
    pub success_labels: bool,
    pub release_gpu_while_idle: bool,
}

#[derive(Debug, Error)]
#[error("the settings are not valid: {0}")]
pub struct InvalidSettings(String);

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_by_default() {
        let settings = Settings::parse("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert!(!settings.detect_windows);
        assert!(!settings.success_labels);
        assert!(!settings.release_gpu_while_idle);
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
        assert!(Settings::parse("").is_err());
        assert!(Settings::parse("null").is_err());
    }

    #[test]
    fn written_settings_read_back_the_same() {
        let settings = Settings {
            detect_windows: true,
            success_labels: false,
            release_gpu_while_idle: true,
        };
        let json = settings.to_json().unwrap();
        assert!(json.contains("\"releaseGpuWhileIdle\": true"), "{json}");
        assert_eq!(Settings::parse(&json).unwrap(), settings);
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
