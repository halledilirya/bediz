use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};

use bediz_core::settings::Theme;
use bediz_platform::{Command, Platform, make_webview_an_app};
use tauri::webview::ScrollBarStyle;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::events::Message;
use crate::settings::Store;
use crate::update;

pub const LABEL: &str = "settings";
pub const UPDATES: &str = "updates";

#[derive(Default)]
pub struct Section(Mutex<Option<&'static str>>);

impl Section {
    pub fn take(&self) -> Option<&'static str> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    fn set(&self, section: Option<&'static str>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = section;
    }
}

pub fn open(
    app: &AppHandle,
    title: &str,
    messages: &Sender<Message>,
    section: Option<&'static str>,
) -> tauri::Result<()> {
    let pending = app.try_state::<Section>();
    if let Some(window) = app.get_webview_window(LABEL) {
        if let (Some(section), Some(pending)) = (section, pending) {
            pending.set(Some(section));
            update::show_section(app);
        }
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    if let Some(pending) = pending {
        pending.set(section);
    }
    let theme = app
        .try_state::<Store>()
        .and_then(|store| window_theme(store.current().theme));
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title(title)
        .theme(theme)
        .inner_size(880.0, 640.0)
        .min_inner_size(640.0, 420.0)
        .center()
        .scroll_bar_style(ScrollBarStyle::FluentOverlay)
        .general_autofill_enabled(false)
        .visible(false)
        .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed)
            && let Some(platform) = handle.try_state::<Platform>()
        {
            platform.send(Command::RecordHotkey(false));
        }
    });
    let messages = messages.clone();
    window.with_webview(move |webview| {
        if let Err(error) = make_webview_an_app(&webview.controller()) {
            let _ = messages.send(Message::Note(format!(
                "the settings window keeps some browser behaviour: {error}"
            )));
        }
    })
}

pub fn retitle(app: &AppHandle, title: &str) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_title(title);
    }
}

// set_theme waits for the main thread, and this runs while the settings are locked.
pub fn show_theme(app: &AppHandle, theme: Theme) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(LABEL) {
            let _ = window.set_theme(window_theme(theme));
        }
    });
}

fn window_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    }
}
