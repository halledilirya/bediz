mod event;

pub use event::{Command, Event, Failure, HotkeyState, Label, MenuTexts};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
pub use windows::{
    FileId, FirstInstance, Platform, Question, Reply, StartError, ask_question, file_id,
    first_instance, make_webview_an_app, pick_folder, preferred_languages, save_chosen,
    save_folder, save_screenshot, synced_by_onedrive,
};
