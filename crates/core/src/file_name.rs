use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub const DEFAULT_PATTERN: &str = "Bediz {date} {time}";
pub const LONGEST_PATTERN: usize = 120;
const LONGEST_NAME: usize = 255;
const TOKENS: [&str; 4] = ["date", "time", "width", "height"];
const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
const DEVICES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
const NUMBERED_DEVICES: [&str; 2] = ["COM", "LPT"];
const DEVICE_NUMBERS: [&str; 13] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "\u{b9}", "\u{b2}", "\u{b3}",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Pattern(String);

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum PatternError {
    #[error("the file name is empty")]
    Empty,
    #[error("the file name is longer than {LONGEST_PATTERN} characters")]
    TooLong,
    #[error("a file name cannot hold {0:?}")]
    Forbidden(char),
    #[error("{0} is not a part a file name can hold")]
    UnknownToken(String),
    #[error("Windows keeps the name {0} for a device")]
    Reserved(String),
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        if text.trim().is_empty() {
            return Err(PatternError::Empty);
        }
        if text.chars().count() > LONGEST_PATTERN {
            return Err(PatternError::TooLong);
        }
        if let Some(forbidden) = text
            .chars()
            .find(|c| FORBIDDEN.contains(c) || c.is_control())
        {
            return Err(PatternError::Forbidden(forbidden));
        }
        for after in text.split('{').skip(1) {
            match after.split_once('}') {
                Some((token, _)) if TOKENS.contains(&token) => {}
                Some((token, _)) => {
                    return Err(PatternError::UnknownToken(format!("{{{token}}}")));
                }
                None => return Err(PatternError::UnknownToken(format!("{{{after}"))),
            }
        }
        if let Some(device) = device_name(text) {
            return Err(PatternError::Reserved(device.to_owned()));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn stem(&self, time: &LocalTime, width: u32, height: u32) -> String {
        let LocalTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        } = *time;
        self.0
            .replace("{date}", &format!("{year:04}-{month:02}-{day:02}"))
            .replace("{time}", &format!("{hour:02}{minute:02}{second:02}"))
            .replace("{width}", &width.to_string())
            .replace("{height}", &height.to_string())
            .trim()
            .to_owned()
    }
}

impl Default for Pattern {
    fn default() -> Self {
        Self(DEFAULT_PATTERN.to_owned())
    }
}

impl TryFrom<String> for Pattern {
    type Error = PatternError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<Pattern> for String {
    fn from(pattern: Pattern) -> Self {
        pattern.0
    }
}

// Windows opens a device instead of a file for these names, even with an extension after them.
fn device_name(text: &str) -> Option<&str> {
    let name = text.trim().split('.').next().unwrap_or_default().trim_end();
    let upper = name.to_uppercase();
    let numbered = NUMBERED_DEVICES.iter().any(|device| {
        upper
            .strip_prefix(device)
            .is_some_and(|number| DEVICE_NUMBERS.contains(&number))
    });
    (DEVICES.contains(&upper.as_str()) || numbered).then_some(name)
}

pub fn png(stem: &str, attempt: u32) -> String {
    numbered(&format!("{stem}.png"), attempt)
}

// Windows allows 255 UTF-16 units in one name, so a stem that fills them gives way to the suffix.
pub fn png_within_limit(stem: &str, attempt: u32) -> String {
    let full = png(stem, attempt);
    let excess = full.encode_utf16().count().saturating_sub(LONGEST_NAME);
    if excess == 0 {
        return full;
    }
    let kept = stem.encode_utf16().count().saturating_sub(excess);
    let mut units = 0;
    let shortened: String = stem
        .chars()
        .take_while(|letter| {
            units += letter.len_utf16();
            units <= kept
        })
        .collect();
    png(&shortened, attempt)
}

pub fn temporary(name: &str) -> String {
    format!("{name}.tmp")
}

pub fn scratch(attempt: u32) -> String {
    temporary(&numbered("~Bediz", attempt))
}

pub fn numbered(name: &str, attempt: u32) -> String {
    if attempt <= 1 {
        return name.to_owned();
    }
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => format!("{stem} ({attempt}).{extension}"),
        _ => format!("{name} ({attempt})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: LocalTime = LocalTime {
        year: 2026,
        month: 3,
        day: 7,
        hour: 9,
        minute: 5,
        second: 2,
    };

    fn pattern(text: &str) -> Pattern {
        Pattern::parse(text).unwrap()
    }

    #[test]
    fn a_screenshot_is_named_by_its_date_and_time() {
        let stem = Pattern::default().stem(&TIME, 1280, 720);
        assert_eq!(stem, "Bediz 2026-03-07 090502");
        assert_eq!(png(&stem, 1), "Bediz 2026-03-07 090502.png");
    }

    #[test]
    fn every_part_of_a_pattern_is_filled_in() {
        let stem = pattern("{width}x{height} on {date} at {time} {date}").stem(&TIME, 1280, 720);
        assert_eq!(stem, "1280x720 on 2026-03-07 at 090502 2026-03-07");
    }

    #[test]
    fn a_pattern_without_parts_is_kept_as_written() {
        assert_eq!(pattern("Notes").stem(&TIME, 1, 1), "Notes");
        assert_eq!(pattern("  Notes  ").stem(&TIME, 1, 1), "Notes");
        assert_eq!(pattern("a}b").stem(&TIME, 1, 1), "a}b");
    }

    #[test]
    fn a_pattern_a_file_name_cannot_hold_is_rejected() {
        assert_eq!(Pattern::parse(""), Err(PatternError::Empty));
        assert_eq!(Pattern::parse("   "), Err(PatternError::Empty));
        assert_eq!(Pattern::parse("a/b"), Err(PatternError::Forbidden('/')));
        assert_eq!(Pattern::parse("a\tb"), Err(PatternError::Forbidden('\t')));
        assert_eq!(Pattern::parse("what?"), Err(PatternError::Forbidden('?')));
        assert_eq!(
            Pattern::parse("{day}"),
            Err(PatternError::UnknownToken("{day}".to_owned()))
        );
        assert_eq!(
            Pattern::parse("Bediz {date"),
            Err(PatternError::UnknownToken("{date".to_owned()))
        );
        assert_eq!(
            Pattern::parse(&"a".repeat(LONGEST_PATTERN + 1)),
            Err(PatternError::TooLong)
        );
        assert!(Pattern::parse(&"ş".repeat(LONGEST_PATTERN)).is_ok());
    }

    #[test]
    fn a_name_windows_keeps_for_a_device_is_rejected() {
        for name in [
            "NUL",
            "con",
            " Aux ",
            "prn.shot",
            "COM1",
            "lpt9",
            "COM\u{b9}",
            "Nul .",
        ] {
            let device = name.trim().split('.').next().unwrap().trim_end();
            assert_eq!(
                Pattern::parse(name),
                Err(PatternError::Reserved(device.to_owned())),
                "{name}"
            );
        }
        for name in [
            "NULL",
            "COM10",
            "LPT",
            "My CON",
            "{date} NUL",
            "NUL{time}",
            "conference",
        ] {
            assert!(Pattern::parse(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn a_pattern_is_stored_as_its_text() {
        let stored = serde_json::to_string(&Pattern::default()).unwrap();
        assert_eq!(stored, r#""Bediz {date} {time}""#);
        assert_eq!(
            serde_json::from_str::<Pattern>(&stored).unwrap(),
            Pattern::default()
        );
        assert!(serde_json::from_str::<Pattern>(r#""a:b""#).is_err());
    }

    #[test]
    fn later_attempts_get_a_number() {
        assert_eq!(png("Notes", 2), "Notes (2).png");
        assert_eq!(png("Notes", 10), "Notes (10).png");
        assert_eq!(png("Notes", 0), png("Notes", 1));
    }

    #[test]
    fn numbers_go_before_the_extension() {
        assert_eq!(numbered("notes.png", 1), "notes.png");
        assert_eq!(numbered("notes.png", 3), "notes (3).png");
        assert_eq!(numbered("archive.tar.png", 2), "archive.tar (2).png");
        assert_eq!(numbered("README", 2), "README (2)");
        assert_eq!(numbered(".png", 2), ".png (2)");
    }

    #[test]
    fn scratch_names_stay_short_whatever_the_final_name() {
        assert_eq!(scratch(1), "~Bediz.tmp");
        assert_eq!(scratch(2), "~Bediz (2).tmp");
        assert!(scratch(1000).len() < 20);
    }

    #[test]
    fn a_temporary_name_keeps_the_final_name_in_front() {
        assert_eq!(temporary(&png("Notes", 1)), "Notes.png.tmp");
    }

    #[test]
    fn a_name_within_the_limit_keeps_its_whole_stem() {
        assert_eq!(png_within_limit("notes", 2), "notes (2).png");
        let longest = "a".repeat(251);
        assert_eq!(png_within_limit(&longest, 1), format!("{longest}.png"));
    }

    #[test]
    fn a_stem_that_fills_the_limit_gives_way_to_the_suffix() {
        let stem = "a".repeat(248);
        let name = png_within_limit(&stem, 2);
        assert_eq!(name, format!("{} (2).png", "a".repeat(247)));
        assert_eq!(name.encode_utf16().count(), 255);
        let name = png_within_limit(&stem, 10_000);
        assert_eq!(name.encode_utf16().count(), 255);
        assert!(name.ends_with(" (10000).png"));
    }

    #[test]
    fn a_shortened_stem_never_splits_a_character() {
        let stem = "\u{1F600}".repeat(124);
        let name = png_within_limit(&stem, 2);
        assert!(name.encode_utf16().count() <= 255);
        assert_eq!(name, format!("{} (2).png", "\u{1F600}".repeat(123)));
    }
}
