use std::path::PathBuf;

use bediz_core::delivery::SaveError;
use bediz_core::settings::Settings;
use bediz_platform::Command;
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

use crate::events::HotkeyStatus;
use crate::i18n::Language;
use crate::settings::{self, Store};
use crate::settings_window::Section;
use crate::setup;
use crate::startup::StartupStatus;
use crate::update::{self, UpdateStatus};
use crate::updater::{self, Updates};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    path: String,
    one_drive: bool,
}

#[tauri::command]
pub fn language(language: State<'_, Language>) -> &'static str {
    language.get()
}

#[tauri::command]
pub fn settings(store: State<'_, Store>) -> (Settings, u64) {
    store.revised()
}

#[tauri::command]
pub fn set_settings(settings: Settings, shown: Settings, store: State<'_, Store>, app: AppHandle) {
    store.change(
        |current| current.with_changes(&shown, &settings),
        |before, after| settings::apply(&app, before, after),
    );
}

#[tauri::command]
pub fn settings_ready(window: WebviewWindow) -> Result<(), String> {
    window
        .show()
        .and_then(|()| window.set_focus())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn take_section(section: State<'_, Section>) -> Option<&'static str> {
    section.take()
}

#[tauri::command]
pub fn hotkey_state(status: State<'_, HotkeyStatus>) -> Option<&'static str> {
    status.get().map(update::state_name)
}

#[tauri::command]
pub fn update_status(updates: State<'_, Updates>) -> UpdateStatus {
    updates.status()
}

#[tauri::command]
pub fn check_for_updates(app: AppHandle) {
    updater::check(&app);
}

#[tauri::command]
pub fn install_update(app: AppHandle) {
    updater::install(&app);
}

#[tauri::command]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
pub async fn open_logs(app: AppHandle) -> Result<(), String> {
    let logs = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("logs");
    in_background(move || {
        app.opener()
            .open_path(logs.to_string_lossy(), None::<&str>)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub fn open_source(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(env!("CARGO_PKG_REPOSITORY"), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_with_windows_problem(status: State<'_, StartupStatus>) -> Option<String> {
    status.problem()
}

#[tauri::command]
pub fn record_hotkey(recording: bool, app: AppHandle) {
    setup::send(&app, Command::RecordHotkey(recording));
}

#[tauri::command]
pub fn take_print_screen(app: AppHandle) {
    setup::send(&app, Command::TakePrintScreen);
}

#[tauri::command]
pub fn retry_hotkey(app: AppHandle) {
    setup::send(&app, Command::RetryHotkey);
}

#[tauri::command]
pub async fn save_folder(chosen: Option<PathBuf>) -> Result<Folder, String> {
    in_background(move || {
        let path = existing_folder(chosen)?;
        Ok(Folder {
            one_drive: bediz_platform::synced_by_onedrive(&path),
            path: path.to_string_lossy().into_owned(),
        })
    })
    .await
}

#[tauri::command]
pub async fn open_save_folder(chosen: Option<PathBuf>, app: AppHandle) -> Result<(), String> {
    in_background(move || {
        let path = existing_folder(chosen)?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn choose_save_folder(
    chosen: Option<PathBuf>,
    window: WebviewWindow,
) -> Result<Option<String>, String> {
    let owner = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
    let picked = in_background(move || {
        let start = resolve(chosen).ok();
        bediz_platform::pick_folder(owner, start).map_err(|error| error.to_string())
    })
    .await?;
    picked
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| "the folder's name cannot be stored".to_owned())
        })
        .transpose()
}

fn resolve(chosen: Option<PathBuf>) -> Result<PathBuf, String> {
    bediz_platform::save_folder(chosen.as_deref()).map_err(|error| error.to_string())
}

// The opener runs whatever it is given, so only a folder that exists is passed on.
fn existing_folder(chosen: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = resolve(chosen)?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("{}: {}", path.display(), SaveError::FolderMissing))
    }
}

// A folder on an unreachable network share can take the network's timeout to answer.
async fn in_background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}
