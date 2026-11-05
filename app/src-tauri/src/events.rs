use std::sync::mpsc::Receiver;

use bediz_platform::{Event, HotkeyState};

use crate::log::Log;
use crate::tray::Tray;

pub fn spawn(events: Receiver<Event>, log: Log, tray: Tray) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("bediz-events".to_owned())
        .spawn(move || {
            for event in events {
                if let Event::Hotkey(state) = event {
                    tray.show(state);
                }
                log.write(&line(&event));
            }
        })?;
    Ok(())
}

fn line(event: &Event) -> String {
    match event {
        Event::Hotkey(state) => format!("hotkey {}", hotkey_name(*state)),
        Event::Idle(memory) => format!("idle {}", memory.log_fields()),
        Event::Measured(measurement) => measurement.log_line(),
        Event::Copied { width, height } => format!("copied {width}x{height}"),
        Event::Saved { width, height } => format!("saved {width}x{height}"),
        Event::CopiedColour => "copied colour".to_owned(),
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
