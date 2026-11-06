use std::sync::mpsc::Sender;

use bediz_platform::{Command, Platform, make_webview_an_app};
use tauri::webview::ScrollBarStyle;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::events::Message;

pub const LABEL: &str = "settings";

pub fn open(app: &AppHandle, title: &str, messages: &Sender<Message>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title(title)
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
