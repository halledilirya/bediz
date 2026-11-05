mod event;

pub use event::{Command, Event, Failure, HotkeyState, Label, MenuTexts};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
pub use windows::{
    FileId, Platform, StartError, file_id, preferred_languages, save_chosen, save_screenshot,
};
