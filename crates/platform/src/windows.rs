//! The Windows platform: screen capture, the overlays, hotkeys, the clipboard, saving, recording,
//! the dialogs and shell calls the app makes, and the thread that owns them.

mod capture;
mod clipboard;
mod clock;
mod contrast;
mod desktop_switch;
mod displays;
mod folder;
mod foreground;
mod gpu;
mod hotkey;
mod label_window;
mod language;
mod memory;
mod overlay;
mod overlay_menu;
mod package;
mod platform;
mod print_screen_setting;
mod question;
mod readback;
mod recording;
mod runtime;
mod save;
mod save_dialog;
mod scene;
mod session;
mod shader;
mod shell;
mod single_instance;
mod startup;
mod virtual_desktop;
mod webview;
mod window_class;
mod window_list;
mod worker;

pub use contrast::apps_use_dark_mode;
pub use folder::{pick_folder, synced_by_onedrive};
pub use language::preferred_languages;
pub use package::{packaged, seen_outside};
pub use platform::{Platform, StartError};
pub use question::{
    Question, QuitChoice, QuitDialog, QuitQuestion, Reply, ask as ask_question, ask_to_quit,
};
pub use save::{
    FileId, SavedAs, file_id, folder as save_folder, keep_beside, replace_own_file, save_beside,
    save_chosen, save_screenshot,
};
pub use shell::{move_to_recycle_bin, show_in_folder};
pub use single_instance::{FirstInstance, first_instance};
pub use startup::start_turned_off_by_user;
pub use webview::make_webview_an_app;
