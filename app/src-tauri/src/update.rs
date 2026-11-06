use bediz_platform::HotkeyState;
use tauri::{AppHandle, Emitter};

use crate::settings_window;

const SETTINGS_NOT_SAVED: &str = "settings-not-saved";
const SETTINGS_CHANGED: &str = "settings-changed";
const HOTKEY_STATE: &str = "hotkey-state";
const HOTKEY_PRESSED: &str = "hotkey-pressed";
const LANGUAGE_CHANGED: &str = "language-changed";
const START_WITH_WINDOWS_PROBLEM: &str = "start-with-windows-problem";

pub fn settings_not_saved(app: &AppHandle, reason: &str) {
    let _ = app.emit_to(settings_window::LABEL, SETTINGS_NOT_SAVED, reason);
}

pub fn settings_changed(app: &AppHandle) {
    let _ = app.emit_to(settings_window::LABEL, SETTINGS_CHANGED, ());
}

pub fn hotkey_state(app: &AppHandle, state: HotkeyState) {
    let _ = app.emit_to(settings_window::LABEL, HOTKEY_STATE, state_name(state));
}

pub fn language_changed(app: &AppHandle) {
    let _ = app.emit_to(settings_window::LABEL, LANGUAGE_CHANGED, ());
}

pub fn hotkey_pressed(app: &AppHandle) {
    let _ = app.emit_to(settings_window::LABEL, HOTKEY_PRESSED, ());
}

pub fn start_with_windows_problem(app: &AppHandle, problem: Option<&str>) {
    let _ = app.emit_to(settings_window::LABEL, START_WITH_WINDOWS_PROBLEM, problem);
}

pub const fn state_name(state: HotkeyState) -> &'static str {
    match state {
        HotkeyState::Registered => "registered",
        HotkeyState::SnippingTool => "snipping-tool",
        HotkeyState::Taken => "taken",
    }
}
