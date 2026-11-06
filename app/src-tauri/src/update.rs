use tauri::{AppHandle, Emitter};

use crate::settings_window;

const SETTINGS_NOT_SAVED: &str = "settings-not-saved";

pub fn settings_not_saved(app: &AppHandle, reason: &str) {
    let _ = app.emit_to(settings_window::LABEL, SETTINGS_NOT_SAVED, reason);
}
