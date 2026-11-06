mod event;

pub use event::{Command, Event, Failure, HotkeyState, Label, MenuTexts};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
pub use windows::{
    FileId, FirstInstance, Platform, StartError, file_id, first_instance, make_webview_an_app,
    preferred_languages, save_chosen, save_screenshot,
};
