use std::sync::mpsc::{Receiver, Sender};

use bediz_core::geometry::Rect;
use bediz_core::notice::{self, Notice};
use bediz_platform::{Command, Event, HotkeyState, Label, Platform};
use tauri::{AppHandle, Manager};

use crate::i18n::Texts;
use crate::log::Log;
use crate::settings::Store;
use crate::settings_window;
use crate::tray::Tray;

pub enum Message {
    Platform(Event),
    OpenSettings,
    Note(String),
}

pub struct Events {
    pub receiver: Receiver<Message>,
    pub sender: Sender<Message>,
}

pub fn spawn(
    events: Events,
    log: Log,
    tray: Tray,
    app: AppHandle,
    languages: Vec<String>,
) -> std::io::Result<()> {
    let Events { receiver, sender } = events;
    std::thread::Builder::new()
        .name("bediz-events".to_owned())
        .spawn(move || {
            let texts = Texts::for_languages(&languages);
            let open_settings = || {
                let title = texts.get("settings-title");
                if let Err(error) = settings_window::open(&app, &title, &sender) {
                    log.write(&format!("failed to open the settings: {error}"));
                }
            };
            for message in receiver {
                let event = match message {
                    Message::Platform(event) => event,
                    Message::OpenSettings => {
                        open_settings();
                        continue;
                    }
                    Message::Note(note) => {
                        log.write(&format!("note {note}"));
                        continue;
                    }
                };
                match &event {
                    Event::Hotkey(state) => tray.show(*state),
                    Event::Notice { notices, near }
                        if notice::worth_a_label(notices, success_labels(&app)) =>
                    {
                        show_label(&app, &texts, notices, *near);
                    }
                    Event::SecondStart => open_settings(),
                    _ => {}
                }
                log.write(&line(&event));
            }
        })?;
    Ok(())
}

fn success_labels(app: &AppHandle) -> bool {
    app.try_state::<Store>()
        .is_some_and(|store| store.current().success_labels)
}

fn show_label(app: &AppHandle, texts: &Texts, notices: &[Notice], near: Rect) {
    let Some(platform) = app.try_state::<Platform>() else {
        return;
    };
    platform.send(Command::ShowLabel(Label {
        lines: notices.iter().map(|notice| texts.notice(notice)).collect(),
        near,
        warning: notices.iter().any(Notice::is_failure),
    }));
}

fn line(event: &Event) -> String {
    match event {
        Event::Hotkey(state) => format!("hotkey {}", hotkey_name(*state)),
        Event::Idle(memory) => format!("idle {}", memory.log_fields()),
        Event::Measured(measurement) => measurement.log_line(),
        Event::Notice { notices, near } => {
            let said: Vec<String> = notices.iter().map(Notice::to_string).collect();
            if near.width() > 1 || near.height() > 1 {
                format!("{} {}x{}", said.join("; "), near.width(), near.height())
            } else {
                said.join("; ")
            }
        }
        Event::Note(note) => format!("note {note}"),
        Event::Failed(failure) => format!("failed {failure}"),
        Event::SecondStart => "second start".to_owned(),
    }
}

fn hotkey_name(state: HotkeyState) -> &'static str {
    match state {
        HotkeyState::Registered => "registered",
        HotkeyState::SnippingTool => "snipping-tool",
        HotkeyState::Taken => "taken",
    }
}
