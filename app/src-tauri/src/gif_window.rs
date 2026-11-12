use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard, PoisonError};

use bediz_core::delivery::{CopyError, SaveError};
use bediz_core::geometry::Rect;
use bediz_core::notice::Notice;
use bediz_core::recording::{SizeWarning, size_warning};
use bediz_core::settings::Theme;
use bediz_platform::{FileId, GifEvent, SavedGif, make_webview_an_app};
use serde::Serialize;
use tauri::webview::ScrollBarStyle;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::events::Message;
use crate::settings::Store;
use crate::settings_window::{background, paint_behind, window_theme};

const LABEL_PREFIX: &str = "gif-";
const CHANGED: &str = "gif-changed";
const LANGUAGE_CHANGED: &str = "language-changed";
const LOOK_CHANGED: &str = "look-changed";
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 400.0;
const FRAME_ALLOWANCE: f64 = 40.0;
const MARGIN: f64 = 12.0;

#[derive(Default)]
pub struct GifState(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    shown: Option<Shown>,
    window: Option<String>,
    opened: u64,
}

struct Shown {
    id: u64,
    status: Status,
}

enum Status {
    Saving(Option<u8>),
    Saved {
        gif: SavedGif,
        identity: Option<FileId>,
        copy: Copy,
    },
    NotSaved(SaveError),
}

#[derive(Clone)]
enum Copy {
    NotYet,
    Done,
    Failed(CopyError),
}

#[derive(Debug, PartialEq, Eq)]
enum Place {
    Open(String),
    New(String),
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    id: &'static str,
    reason: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum CopyView {
    NotYet,
    Done,
    Failed { problem: Problem },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum View {
    Empty,
    Saving {
        id: u64,
        percent: Option<u8>,
    },
    Saved {
        id: u64,
        name: String,
        bytes: u64,
        milliseconds: u64,
        width: u32,
        height: u32,
        warning: Option<SizeWarning>,
        copy: CopyView,
    },
    NotSaved {
        id: u64,
        problem: Problem,
    },
}

pub struct Target {
    pub path: PathBuf,
    pub identity: Option<FileId>,
}

impl GifState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn view(&self) -> View {
        let inner = self.lock();
        let Some(Shown { id, status }) = inner.shown.as_ref() else {
            return View::Empty;
        };
        match status {
            Status::Saving(percent) => View::Saving {
                id: *id,
                percent: *percent,
            },
            Status::Saved { gif, copy, .. } => saved_view(*id, gif, copy),
            Status::NotSaved(error) => View::NotSaved {
                id: *id,
                problem: problem(&Notice::SaveFailed(error.clone())),
            },
        }
    }

    pub fn target(&self, id: u64) -> Option<Target> {
        match self.lock().shown.as_ref() {
            Some(Shown {
                id: shown,
                status: Status::Saved { gif, identity, .. },
            }) if *shown == id => Some(Target {
                path: gif.path.clone(),
                identity: *identity,
            }),
            _ => None,
        }
    }

    fn start(&self, id: u64, exists: impl Fn(&str) -> bool) -> Place {
        let mut inner = self.lock();
        inner.shown = Some(Shown {
            id,
            status: Status::Saving(None),
        });
        if let Some(label) = inner.window.clone().filter(|label| exists(label)) {
            return Place::Open(label);
        }
        inner.opened = inner.opened.wrapping_add(1);
        let label = format!("{LABEL_PREFIX}{}", inner.opened);
        inner.window = Some(label.clone());
        Place::New(label)
    }

    fn update(&self, id: u64, change: impl FnOnce(&mut Status)) -> Option<String> {
        let mut inner = self.lock();
        let Inner { shown, window, .. } = &mut *inner;
        match shown {
            Some(shown) if shown.id == id => {
                change(&mut shown.status);
                window.clone()
            }
            _ => None,
        }
    }

    fn closed(&self, label: &str) {
        let mut inner = self.lock();
        if inner.window.as_deref() == Some(label) {
            inner.window = None;
            inner.shown = None;
        }
    }

    fn take_window_of(&self, id: u64) -> Option<String> {
        let mut inner = self.lock();
        if inner.shown.as_ref().is_some_and(|shown| shown.id == id) {
            inner.shown = None;
            inner.window.take()
        } else {
            None
        }
    }

    fn window(&self) -> Option<String> {
        self.lock().window.clone()
    }
}

pub fn on_event(app: &AppHandle, event: &GifEvent, title: &str, messages: &Sender<Message>) {
    let Some(state) = app.try_state::<GifState>() else {
        return;
    };
    let changed = match event {
        GifEvent::Saving { id, near } => {
            if !wanted(app) {
                return;
            }
            match state.start(*id, |label| app.get_webview_window(label).is_some()) {
                Place::Open(label) => Some(label),
                Place::New(label) => {
                    if let Err(error) = open(app, &label, *near, title, messages) {
                        state.closed(&label);
                        let _ = messages.send(Message::Note(format!(
                            "the window for the GIF could not be opened: {error}"
                        )));
                    }
                    None
                }
            }
        }
        GifEvent::Written { id, percent } => state.update(*id, |status| {
            if let Status::Saving(shown) = status {
                *shown = Some(*percent);
            }
        }),
        GifEvent::Saved { id, gif } => {
            let identity = bediz_platform::file_id(&gif.path).ok();
            state.update(*id, |status| {
                *status = Status::Saved {
                    gif: gif.clone(),
                    identity,
                    copy: Copy::NotYet,
                }
            })
        }
        GifEvent::NotSaved { id, error } => state.update(*id, |status| {
            *status = Status::NotSaved(error.clone());
        }),
        GifEvent::Copied { id, result } => state.update(*id, |status| {
            if let Status::Saved { copy, .. } = status {
                *copy = match result {
                    Ok(()) => Copy::Done,
                    Err(error) => Copy::Failed(error.clone()),
                };
            }
        }),
    };
    if let Some(label) = changed {
        let _ = app.emit_to(label, CHANGED, ());
    }
}

pub fn language_changed(app: &AppHandle, title: &str) {
    let Some(label) = app.try_state::<GifState>().and_then(|state| state.window()) else {
        return;
    };
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.set_title(title);
        let _ = app.emit_to(label, LANGUAGE_CHANGED, ());
    }
}

// set_theme waits for the main thread, and this runs while the settings are locked.
pub fn show_look(app: &AppHandle, theme: Theme) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(label) = handle
            .try_state::<GifState>()
            .and_then(|state| state.window())
        else {
            return;
        };
        if let Some(window) = handle.get_webview_window(&label) {
            let _ = window.set_theme(window_theme(theme));
            paint_behind(&window);
            let _ = handle.emit_to(label, LOOK_CHANGED, ());
        }
    });
}

pub fn deleted(app: &AppHandle, id: u64) {
    let label = app
        .try_state::<GifState>()
        .and_then(|state| state.take_window_of(id));
    if let Some(window) = label.and_then(|label| app.get_webview_window(&label)) {
        let _ = window.destroy();
    }
}

fn wanted(app: &AppHandle) -> bool {
    app.try_state::<Store>()
        .is_none_or(|store| store.current().gif_window)
}

fn open(
    app: &AppHandle,
    label: &str,
    near: Rect,
    title: &str,
    messages: &Sender<Message>,
) -> tauri::Result<()> {
    let theme = app
        .try_state::<Store>()
        .and_then(|store| window_theme(store.current().theme));
    let dark = match theme {
        Some(tauri::Theme::Dark) => true,
        Some(_) => false,
        None => bediz_platform::apps_use_dark_mode(),
    };
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("gif.html".into()))
        .title(title)
        .theme(theme)
        .background_color(background(dark))
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .always_on_top(true)
        .focused(false)
        .scroll_bar_style(ScrollBarStyle::FluentOverlay)
        .general_autofill_enabled(false);
    if let Some((x, y)) = corner(app, near) {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    let handle = app.clone();
    let own = label.to_owned();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
            if let Some(state) = handle.try_state::<GifState>() {
                state.closed(&own);
            }
        }
        WindowEvent::ThemeChanged(_) => {
            if let Some(window) = handle.get_webview_window(&own) {
                paint_behind(&window);
            }
        }
        _ => {}
    });
    let messages = messages.clone();
    window.with_webview(move |webview| {
        if let Err(error) = make_webview_an_app(&webview.controller()) {
            let _ = messages.send(Message::Note(format!(
                "the GIF window keeps some browser behaviour: {error}"
            )));
        }
    })
}

// Tao reads a starting position in the scale of the monitor it falls on, so it is given in that one's.
fn corner(app: &AppHandle, near: Rect) -> Option<(f64, f64)> {
    let x = f64::from(near.left) + f64::from(near.width()) / 2.0;
    let y = f64::from(near.top) + f64::from(near.height()) / 2.0;
    let monitor = app
        .monitor_from_point(x, y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let right = f64::from(work.position.x) + f64::from(work.size.width);
    let bottom = f64::from(work.position.y) + f64::from(work.size.height);
    Some((
        right / scale - WIDTH - MARGIN,
        bottom / scale - HEIGHT - FRAME_ALLOWANCE - MARGIN,
    ))
}

fn saved_view(id: u64, gif: &SavedGif, copy: &Copy) -> View {
    View::Saved {
        id,
        name: gif
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        bytes: gif.bytes,
        milliseconds: u64::try_from(gif.length.as_millis()).unwrap_or(u64::MAX),
        width: gif.width,
        height: gif.height,
        warning: size_warning(gif.bytes),
        copy: match copy {
            Copy::NotYet => CopyView::NotYet,
            Copy::Done => CopyView::Done,
            Copy::Failed(error) => CopyView::Failed {
                problem: problem(&Notice::CopyFailed(error.clone())),
            },
        },
    }
}

fn problem(notice: &Notice) -> Problem {
    Problem {
        id: notice.locale_id(),
        reason: notice.argument().map(|(_, value)| value),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    fn saved(bytes: u64) -> SavedGif {
        SavedGif {
            path: PathBuf::from(r"C:\GIFs\Bediz 2026-11-12 640x360.gif"),
            bytes,
            length: Duration::from_millis(12_345),
            width: 640,
            height: 360,
        }
    }

    fn mark_saved(state: &GifState, id: u64) -> Option<String> {
        state.update(id, |status| {
            *status = Status::Saved {
                gif: saved(5),
                identity: None,
                copy: Copy::NotYet,
            }
        })
    }

    fn open(_: &str) -> bool {
        true
    }

    #[test]
    fn a_saved_gif_reaches_the_window_with_its_facts_and_no_folder() {
        let shown = serde_json::to_value(saved_view(3, &saved(12_000_000), &Copy::Done)).unwrap();
        assert_eq!(
            shown,
            json!({
                "state": "saved",
                "id": 3,
                "name": "Bediz 2026-11-12 640x360.gif",
                "bytes": 12_000_000,
                "milliseconds": 12_345,
                "width": 640,
                "height": 360,
                "warning": "gitHub",
                "copy": { "state": "done" },
            })
        );
    }

    #[test]
    fn a_gif_being_written_shows_how_far_it_got() {
        let state = GifState::default();
        state.start(4, open);
        assert_eq!(
            serde_json::to_value(state.view()).unwrap(),
            json!({ "state": "saving", "id": 4, "percent": null })
        );
        assert!(
            state
                .update(4, |status| *status = Status::Saving(Some(40)))
                .is_some()
        );
        assert_eq!(
            state.view(),
            View::Saving {
                id: 4,
                percent: Some(40)
            }
        );
    }

    #[test]
    fn a_problem_is_passed_as_its_message_and_reason() {
        let copy = Copy::Failed(CopyError::Other("Zugriff verweigert".to_owned()));
        let View::Saved { copy, warning, .. } = saved_view(1, &saved(1), &copy) else {
            panic!("a saved GIF is not shown as saved");
        };
        assert_eq!(warning, None);
        assert_eq!(
            copy,
            CopyView::Failed {
                problem: Problem {
                    id: "copy-error-other",
                    reason: Some("Zugriff verweigert".to_owned()),
                }
            }
        );
        assert_eq!(
            problem(&Notice::SaveFailed(SaveError::DiskFull)),
            Problem {
                id: "save-error-disk-full",
                reason: None,
            }
        );
    }

    #[test]
    fn only_the_gif_being_shown_is_changed_or_handed_out() {
        let state = GifState::default();
        assert_eq!(state.view(), View::Empty);
        assert_eq!(state.start(2, open), Place::New("gif-1".to_owned()));
        assert_eq!(
            state.view(),
            View::Saving {
                id: 2,
                percent: None
            }
        );
        assert!(
            state
                .update(1, |status| *status = Status::NotSaved(SaveError::DiskFull))
                .is_none()
        );
        assert!(state.target(2).is_none());
        assert_eq!(mark_saved(&state, 2), Some("gif-1".to_owned()));
        assert!(state.target(1).is_none());
        assert_eq!(state.target(2).unwrap().path, saved(5).path);
    }

    #[test]
    fn a_new_recording_takes_over_the_open_window() {
        let state = GifState::default();
        state.start(1, open);
        mark_saved(&state, 1);
        assert_eq!(state.start(2, open), Place::Open("gif-1".to_owned()));
        assert!(state.target(1).is_none());
        assert_eq!(mark_saved(&state, 1), None);
    }

    #[test]
    fn deleting_a_gif_no_longer_shown_leaves_the_new_one_and_its_window() {
        let state = GifState::default();
        state.start(1, open);
        mark_saved(&state, 1);
        state.start(2, open);
        assert_eq!(state.take_window_of(1), None);
        assert!(matches!(state.view(), View::Saving { id: 2, .. }));
        assert_eq!(state.window(), Some("gif-1".to_owned()));
    }

    #[test]
    fn deleting_the_gif_shown_frees_its_window_before_the_next_recording() {
        let state = GifState::default();
        state.start(1, open);
        mark_saved(&state, 1);
        assert_eq!(state.take_window_of(1), Some("gif-1".to_owned()));
        assert_eq!(state.view(), View::Empty);
        assert_eq!(state.start(2, open), Place::New("gif-2".to_owned()));
        state.closed("gif-1");
        assert!(matches!(state.view(), View::Saving { id: 2, .. }));
        assert_eq!(state.window(), Some("gif-2".to_owned()));
    }

    #[test]
    fn a_window_closed_by_the_user_forgets_its_gif() {
        let state = GifState::default();
        state.start(1, open);
        state.closed("gif-1");
        assert_eq!(state.view(), View::Empty);
        assert_eq!(state.window(), None);
        assert_eq!(state.start(2, open), Place::New("gif-2".to_owned()));
    }

    #[test]
    fn a_window_gone_without_a_word_is_replaced() {
        let state = GifState::default();
        state.start(1, open);
        assert_eq!(state.start(2, |_| false), Place::New("gif-2".to_owned()));
        state.closed("gif-1");
        assert!(matches!(state.view(), View::Saving { id: 2, .. }));
    }
}
