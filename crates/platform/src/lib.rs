mod event;

pub use event::{Command, Event, Failure, HotkeyState, Label};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
pub use windows::{Platform, StartError, preferred_languages, save_screenshot};
