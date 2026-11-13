//! Hotkeys as the settings store them by name, and the rule that a typing key needs Ctrl or Alt.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Letter(char),
    Digit(u8),
    Function(u8),
    PrintScreen,
    Pause,
    ScrollLock,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Space,
}

const NAMED: [(Key, &str); 14] = [
    (Key::PrintScreen, "Print Screen"),
    (Key::Pause, "Pause"),
    (Key::ScrollLock, "Scroll Lock"),
    (Key::Insert, "Insert"),
    (Key::Delete, "Delete"),
    (Key::Home, "Home"),
    (Key::End, "End"),
    (Key::PageUp, "Page Up"),
    (Key::PageDown, "Page Down"),
    (Key::Up, "Up"),
    (Key::Down, "Down"),
    (Key::Left, "Left"),
    (Key::Right, "Right"),
    (Key::Space, "Space"),
];

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum HotkeyError {
    #[error("{0:?} is not a key a shortcut can use")]
    UnknownKey(String),
    #[error("{0:?} is not Ctrl, Alt or Shift")]
    UnknownModifier(String),
    #[error("{0} would take that key from typing in every app; add Ctrl or Alt")]
    NeedsCtrlOrAlt(String),
}

impl Key {
    pub const fn may_stand_alone(self) -> bool {
        matches!(
            self,
            Self::Function(_) | Self::PrintScreen | Self::Pause | Self::ScrollLock
        )
    }
}

impl Hotkey {
    pub const PRINT_SCREEN: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        key: Key::PrintScreen,
    };

    fn checked(self) -> Result<Self, HotkeyError> {
        if self.ctrl || self.alt || self.key.may_stand_alone() {
            Ok(self)
        } else {
            Err(HotkeyError::NeedsCtrlOrAlt(self.to_string()))
        }
    }
}

impl Default for Hotkey {
    fn default() -> Self {
        Self::PRINT_SCREEN
    }
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Letter(letter) => write!(formatter, "{letter}"),
            Self::Digit(digit) => write!(formatter, "{digit}"),
            Self::Function(number) => write!(formatter, "F{number}"),
            named => {
                let name = NAMED
                    .iter()
                    .find(|(key, _)| key == named)
                    .map_or("", |(_, name)| name);
                formatter.write_str(name)
            }
        }
    }
}

impl FromStr for Key {
    type Err = HotkeyError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if let Some((key, _)) = NAMED.iter().find(|(_, named)| *named == name) {
            return Ok(*key);
        }
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
            (Some(letter @ 'A'..='Z'), None) => return Ok(Self::Letter(letter)),
            (Some(digit @ '0'..='9'), None) => {
                return Ok(Self::Digit(digit.to_digit(10).map_or(0, |d| d as u8)));
            }
            _ => {}
        }
        name.strip_prefix('F')
            .filter(|number| !number.starts_with('0'))
            .and_then(|number| number.parse::<u8>().ok())
            .filter(|number| (1..=24).contains(number))
            .map(Self::Function)
            .ok_or_else(|| HotkeyError::UnknownKey(name.to_owned()))
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let held = [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ];
        for (down, name) in held {
            if down {
                formatter.write_str(name)?;
            }
        }
        write!(formatter, "{}", self.key)
    }
}

impl FromStr for Hotkey {
    type Err = HotkeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (modifiers, key) = text.rsplit_once('+').unwrap_or(("", text));
        let mut hotkey = Self {
            ctrl: false,
            alt: false,
            shift: false,
            key: key.parse()?,
        };
        for modifier in modifiers.split('+').filter(|part| !part.is_empty()) {
            match modifier {
                "Ctrl" => hotkey.ctrl = true,
                "Alt" => hotkey.alt = true,
                "Shift" => hotkey.shift = true,
                other => return Err(HotkeyError::UnknownModifier(other.to_owned())),
            }
        }
        hotkey.checked()
    }
}

impl TryFrom<String> for Hotkey {
    type Error = HotkeyError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Hotkey> for String {
    fn from(hotkey: Hotkey) -> Self {
        hotkey.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Result<Hotkey, HotkeyError> {
        text.parse()
    }

    #[test]
    fn print_screen_stands_alone() {
        assert_eq!(parsed("Print Screen"), Ok(Hotkey::PRINT_SCREEN));
        assert_eq!(Hotkey::PRINT_SCREEN.to_string(), "Print Screen");
        assert_eq!(Hotkey::default(), Hotkey::PRINT_SCREEN);
    }

    #[test]
    fn modifiers_are_written_in_one_order() {
        let hotkey = parsed("Shift+Ctrl+A").unwrap();
        assert!(hotkey.ctrl && hotkey.shift && !hotkey.alt);
        assert_eq!(hotkey.key, Key::Letter('A'));
        assert_eq!(hotkey.to_string(), "Ctrl+Shift+A");
    }

    #[test]
    fn every_key_reads_back_the_same() {
        let mut keys: Vec<Key> = NAMED.iter().map(|(key, _)| *key).collect();
        keys.extend(('A'..='Z').map(Key::Letter));
        keys.extend((0..=9).map(Key::Digit));
        keys.extend((1..=24).map(Key::Function));
        for key in keys {
            let hotkey = Hotkey {
                ctrl: true,
                alt: true,
                shift: true,
                key,
            };
            assert_eq!(parsed(&hotkey.to_string()), Ok(hotkey), "{hotkey}");
        }
    }

    #[test]
    fn a_typing_key_needs_ctrl_or_alt() {
        for text in ["A", "7", "Space", "Home", "Up", "Shift+A", "Shift+Delete"] {
            assert!(
                matches!(parsed(text), Err(HotkeyError::NeedsCtrlOrAlt(_))),
                "{text}"
            );
        }
        for text in ["Ctrl+A", "Alt+7", "Ctrl+Shift+Space", "Alt+Home"] {
            assert!(parsed(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn function_keys_and_print_screen_may_stand_alone() {
        for text in [
            "F1",
            "F24",
            "Pause",
            "Scroll Lock",
            "Shift+F5",
            "Shift+Print Screen",
        ] {
            assert!(parsed(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn unknown_keys_and_modifiers_are_rejected() {
        for text in [
            "",
            "a",
            "F0",
            "F25",
            "F05",
            "Esc",
            "PrintScreen",
            "Ctrl+",
            "AB",
        ] {
            assert!(
                matches!(parsed(text), Err(HotkeyError::UnknownKey(_))),
                "{text}"
            );
        }
        assert_eq!(
            parsed("Win+A"),
            Err(HotkeyError::UnknownModifier("Win".to_owned()))
        );
    }

    #[test]
    fn a_hotkey_is_stored_as_its_name() {
        let hotkey = parsed("Ctrl+Alt+F9").unwrap();
        assert_eq!(serde_json::to_string(&hotkey).unwrap(), r#""Ctrl+Alt+F9""#);
        assert_eq!(
            serde_json::from_str::<Hotkey>(r#""Ctrl+Alt+F9""#).unwrap(),
            hotkey
        );
        assert!(serde_json::from_str::<Hotkey>(r#""Q""#).is_err());
    }
}
