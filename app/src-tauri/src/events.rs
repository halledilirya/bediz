use std::sync::mpsc::Receiver;

use bediz_core::geometry::Rect;
use bediz_core::notice::{self, Notice};
use bediz_platform::{Command, Event, HotkeyState, Label, Platform};
use tauri::{AppHandle, Manager};

use crate::i18n::Texts;
use crate::log::Log;
use crate::tray::Tray;

pub fn spawn(
    events: Receiver<Event>,
    log: Log,
    tray: Tray,
    app: AppHandle,
    languages: Vec<String>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("bediz-events".to_owned())
        .spawn(move || {
            let texts = Texts::for_languages(&languages);
            let confirmations = false;
            for event in events {
                match &event {
                    Event::Hotkey(state) => tray.show(*state),
                    Event::Notice { notices, near }
                        if notice::worth_a_label(notices, confirmations) =>
                    {
                        show_label(&app, &texts, notices, *near);
                    }
                    _ => {}
                }
                log.write(&line(&event));
            }
        })?;
    Ok(())
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
    }
}

fn hotkey_name(state: HotkeyState) -> &'static str {
    match state {
        HotkeyState::Registered => "registered",
        HotkeyState::SnippingTool => "snipping-tool",
        HotkeyState::Taken => "taken",
    }
}
