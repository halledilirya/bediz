use std::path::PathBuf;

use bediz_core::delivery::SaveError;
use bediz_core::settings::Settings;
use bediz_platform::{Command, Platform};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

use crate::events::HotkeyStatus;
use crate::i18n::Language;
use crate::settings::{self, Store};
use crate::startup::StartupStatus;
use crate::update;

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
pub fn set_settings(settings: Settings, store: State<'_, Store>, app: AppHandle) {
    store.change(
        |current| settings.keeping_facts_of(current),
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
pub fn hotkey_state(status: State<'_, HotkeyStatus>) -> Option<&'static str> {
    status.get().map(update::state_name)
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
    send(&app, Command::RecordHotkey(recording));
}

#[tauri::command]
pub fn take_print_screen(app: AppHandle) {
    send(&app, Command::TakePrintScreen);
}

#[tauri::command]
pub fn retry_hotkey(app: AppHandle) {
    send(&app, Command::RetryHotkey);
}

#[tauri::command]
pub async fn save_folder(chosen: Option<PathBuf>) -> Result<Folder, String> {
    in_background(move || {
        let path = resolve(chosen)?;
        if !path.is_dir() {
            return Err(format!("{}: {}", path.display(), SaveError::FolderMissing));
        }
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
        let path = resolve(chosen)?;
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

// A folder on an unreachable network share can take the network's timeout to answer.
async fn in_background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}

fn send(app: &AppHandle, command: Command) {
    if let Some(platform) = app.try_state::<Platform>() {
        platform.send(command);
    }
}
