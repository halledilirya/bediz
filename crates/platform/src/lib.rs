mod event;

pub use event::{
    Command, Event, Failure, HotkeyState, Label, MenuTexts, RecordingAction, RecordingState,
    Shortcut,
};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
pub use windows::{
    FileId, FirstInstance, Platform, Question, Reply, SavedAs, StartError, apps_use_dark_mode,
    ask_question, file_id, first_instance, keep_beside, make_webview_an_app, packaged, pick_folder,
    preferred_languages, replace_own_file, save_beside, save_chosen, save_folder, save_screenshot,
    seen_outside, start_turned_off_by_user, synced_by_onedrive,
};
