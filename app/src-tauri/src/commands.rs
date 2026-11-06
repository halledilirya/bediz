use bediz_core::settings::Settings;
use bediz_platform::Platform;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::i18n::Language;
use crate::settings::{self, Store};

#[tauri::command]
pub fn language(language: State<'_, Language>) -> &'static str {
    language.0
}

#[tauri::command]
pub fn settings(store: State<'_, Store>) -> (Settings, u64) {
    store.revised()
}

#[tauri::command]
pub fn set_settings(settings: Settings, store: State<'_, Store>, app: AppHandle) {
    store.change(settings, |wanted| {
        if let Some(platform) = app.try_state::<Platform>() {
            settings::apply(&platform, wanted);
        }
    });
}

#[tauri::command]
pub fn settings_ready(window: WebviewWindow) -> Result<(), String> {
    window
        .show()
        .and_then(|()| window.set_focus())
        .map_err(|error| error.to_string())
}
