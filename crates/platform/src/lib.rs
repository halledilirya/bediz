mod event;

pub use event::{Command, Event, Failure, HotkeyState, Label, MenuTexts};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
pub use windows::{
    FileId, FirstInstance, Platform, Question, Reply, SavedAs, StartError, ask_question, file_id,
    first_instance, keep_beside, make_webview_an_app, pick_folder, preferred_languages,
    replace_own_file, save_beside, save_chosen, save_folder, save_screenshot, synced_by_onedrive,
};
