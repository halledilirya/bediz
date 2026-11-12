use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};

use bediz_core::delivery::SaveError;
use bediz_core::settings::Settings;
use bediz_platform::{Command, Shortcut};
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

use crate::events::{HotkeyStatus, Message, Messages};
use crate::gif_window::{self, GifState, Target, View};
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

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Failure {
    Missing { path: String },
    Changed,
    NotShown,
    Windows { reason: String },
    Unexpected,
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
pub fn take_section(section: State<'_, Section>) -> Option<&'static str> {
    section.take()
}

#[tauri::command]
pub fn hotkey_state(status: State<'_, HotkeyStatus>) -> Option<&'static str> {
    status.get(Shortcut::Region).map(update::state_name)
}

#[tauri::command]
pub fn gif_hotkey_state(status: State<'_, HotkeyStatus>) -> Option<&'static str> {
    status.get(Shortcut::Gif).map(update::state_name)
}

#[tauri::command]
pub fn updates_offered() -> bool {
    updater::offered()
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
pub async fn open_logs(app: AppHandle) -> Result<(), Failure> {
    let logs = app
        .path()
        .app_local_data_dir()
        .map_err(|error| unexpected(&app, error))?
        .join("logs");
    in_background(move || {
        let logs = bediz_platform::seen_outside(&logs);
        app.opener()
            .open_path(logs.to_string_lossy(), None::<&str>)
            .map_err(|error| unexpected(&app, error))
    })
    .await
}

#[tauri::command]
pub fn open_source(app: AppHandle) -> Result<(), Failure> {
    app.opener()
        .open_url(env!("CARGO_PKG_REPOSITORY"), None::<&str>)
        .map_err(|error| unexpected(&app, error))
}

#[tauri::command]
pub fn start_with_windows_problem(status: State<'_, StartupStatus>) -> Option<String> {
    status.problem()
}

#[tauri::command]
pub async fn start_turned_off_by_user(app: AppHandle) -> Result<bool, Failure> {
    in_background(move || {
        bediz_platform::start_turned_off_by_user().map_err(|error| unexpected(&app, error))
    })
    .await
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
pub async fn save_folder(chosen: Option<PathBuf>, app: AppHandle) -> Result<Folder, Failure> {
    in_background(move || {
        let path = existing_folder(&app, chosen)?;
        Ok(Folder {
            one_drive: bediz_platform::synced_by_onedrive(&path),
            path: path.to_string_lossy().into_owned(),
        })
    })
    .await
}

#[tauri::command]
pub async fn open_save_folder(chosen: Option<PathBuf>, app: AppHandle) -> Result<(), Failure> {
    in_background(move || {
        let path = existing_folder(&app, chosen)?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|error| unexpected(&app, error))
    })
    .await
}

#[tauri::command]
pub async fn choose_save_folder(
    chosen: Option<PathBuf>,
    window: WebviewWindow,
) -> Result<Option<String>, Failure> {
    let app = window.app_handle().clone();
    let owner = window.hwnd().map_err(|error| unexpected(&app, error))?.0 as isize;
    let picked = in_background(move || {
        let start = bediz_platform::save_folder(chosen.as_deref()).ok();
        bediz_platform::pick_folder(owner, start).map_err(|error| Failure::Windows {
            reason: error.to_string(),
        })
    })
    .await?;
    picked
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| unexpected(&app, "the folder's name cannot be stored"))
        })
        .transpose()
}

#[tauri::command]
pub fn gif(state: State<'_, GifState>) -> View {
    state.view()
}

#[tauri::command]
pub async fn gif_preview(id: u64, app: AppHandle) -> Result<Response, Failure> {
    let target = shown_gif(&app, id)?;
    in_background(move || {
        std::fs::read(&target.path)
            .map(Response::new)
            .map_err(|error| file_failure(&target.path, &error))
    })
    .await
}

#[tauri::command]
pub async fn copy_gif(id: u64, app: AppHandle) -> Result<(), Failure> {
    let target = shown_gif(&app, id)?;
    setup::send(&app, Command::ReserveGifCopy { id });
    in_background(move || {
        still_ours(&target)?;
        setup::send(
            &app,
            Command::CopyGif {
                id,
                path: target.path,
            },
        );
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn show_gif(id: u64, app: AppHandle) -> Result<(), Failure> {
    let target = shown_gif(&app, id)?;
    in_background(move || {
        still_there(&target.path)?;
        bediz_platform::show_in_folder(&target.path)
            .map_err(|error| file_failure(&target.path, &error))
    })
    .await
}

// The opener runs whatever it is given, so only the GIF Bediz saved is passed on.
#[tauri::command]
pub async fn open_gif(id: u64, app: AppHandle) -> Result<(), Failure> {
    let target = shown_gif(&app, id)?;
    in_background(move || {
        still_ours(&target)?;
        app.opener()
            .open_path(target.path.to_string_lossy(), None::<&str>)
            .map_err(|error| unexpected(&app, error))
    })
    .await
}

#[tauri::command]
pub async fn delete_gif(id: u64, window: WebviewWindow) -> Result<bool, Failure> {
    let app = window.app_handle().clone();
    let owner = window.hwnd().map_err(|error| unexpected(&app, error))?.0 as isize;
    let target = shown_gif(&app, id)?;
    let deleted = in_background(move || {
        still_ours(&target)?;
        bediz_platform::move_to_recycle_bin(&target.path, owner)
            .map_err(|error| file_failure(&target.path, &error))
    })
    .await?;
    if deleted {
        gif_window::deleted(&app, id);
    }
    Ok(deleted)
}

fn shown_gif(app: &AppHandle, id: u64) -> Result<Target, Failure> {
    app.try_state::<GifState>()
        .and_then(|state| state.target(id))
        .ok_or(Failure::NotShown)
}

fn still_there(path: &Path) -> Result<(), Failure> {
    if path.is_file() {
        Ok(())
    } else {
        Err(Failure::Missing {
            path: path.display().to_string(),
        })
    }
}

// Another file can take the GIF's name after it is moved away, and it is never copied, opened or deleted.
fn still_ours(target: &Target) -> Result<(), Failure> {
    still_there(&target.path)?;
    let found = bediz_platform::file_id(&target.path)
        .map_err(|error| file_failure(&target.path, &error))?;
    if target.identity == Some(found) {
        Ok(())
    } else {
        Err(Failure::Changed)
    }
}

fn file_failure(path: &Path, error: &io::Error) -> Failure {
    if error.kind() == io::ErrorKind::NotFound {
        Failure::Missing {
            path: path.display().to_string(),
        }
    } else {
        Failure::Windows {
            reason: error.to_string(),
        }
    }
}

fn resolve(app: &AppHandle, chosen: Option<PathBuf>) -> Result<PathBuf, Failure> {
    bediz_platform::save_folder(chosen.as_deref()).map_err(|error| match error {
        SaveError::FolderMissing => Failure::Missing {
            path: chosen
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        },
        SaveError::Other(reason) => Failure::Windows { reason },
        other => unexpected(app, other),
    })
}

// The opener runs whatever it is given, so only a folder that exists is passed on.
fn existing_folder(app: &AppHandle, chosen: Option<PathBuf>) -> Result<PathBuf, Failure> {
    let path = resolve(app, chosen)?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(Failure::Missing {
            path: path.display().to_string(),
        })
    }
}

// Our own details are English, so they go to the log and the window says it in the user's language.
fn unexpected(app: &AppHandle, detail: impl Display) -> Failure {
    if let Some(messages) = app.try_state::<Messages>() {
        let _ = messages.0.send(Message::Note(format!(
            "a window of Bediz could not do that: {detail}"
        )));
    }
    Failure::Unexpected
}

// A folder on an unreachable network share can take the network's timeout to answer.
async fn in_background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or(Err(Failure::Unexpected))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Failure;

    #[test]
    fn a_failure_reaches_the_window_as_its_kind_with_no_english_detail() {
        let shown = |failure: Failure| serde_json::to_value(failure).unwrap();
        assert_eq!(
            shown(Failure::Missing {
                path: r"C:\Users\Ada\Pictures\2.png".to_owned()
            }),
            json!({ "kind": "missing", "path": r"C:\Users\Ada\Pictures\2.png" })
        );
        assert_eq!(
            shown(Failure::Windows {
                reason: "Erişim engellendi.".to_owned()
            }),
            json!({ "kind": "windows", "reason": "Erişim engellendi." })
        );
        assert_eq!(shown(Failure::Unexpected), json!({ "kind": "unexpected" }));
        assert_eq!(shown(Failure::Changed), json!({ "kind": "changed" }));
        assert_eq!(shown(Failure::NotShown), json!({ "kind": "notShown" }));
    }
}
