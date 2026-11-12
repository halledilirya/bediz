mod event;

pub use event::{
    Command, Event, Failure, GifEvent, HotkeyState, Label, MenuTexts, QuitStep, RecordingAction,
    RecordingState, SavedGif, Shortcut,
};

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(windows)]
pub use windows::{
    FileId, FirstInstance, Platform, Question, QuitChoice, QuitDialog, QuitQuestion, Reply,
    SavedAs, StartError, apps_use_dark_mode, ask_question, ask_to_quit, file_id, first_instance,
    keep_beside, make_webview_an_app, move_to_recycle_bin, packaged, pick_folder,
    preferred_languages, replace_own_file, save_beside, save_chosen, save_folder, save_screenshot,
    seen_outside, show_in_folder, start_turned_off_by_user, synced_by_onedrive,
};
