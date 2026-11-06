use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, PoisonError};

use bediz_core::geometry::Rect;
use bediz_core::notice::{self, Notice};
use bediz_core::settings::Settings;
use bediz_platform::{Command, Event, HotkeyState, Label, Platform};
use tauri::{AppHandle, Manager};

use crate::i18n::Texts;
use crate::log::Log;
use crate::question;
use crate::settings::{self, Store};
use crate::settings_window;
use crate::startup::StartupStatus;
use crate::tray::Tray;
use crate::update;

pub enum Message {
    Platform(Event),
    OpenSettings,
    Note(String),
}

pub struct Events {
    pub receiver: Receiver<Message>,
    pub sender: Sender<Message>,
}

#[derive(Default)]
pub struct HotkeyStatus(Mutex<Option<HotkeyState>>);

impl HotkeyStatus {
    pub fn get(&self) -> Option<HotkeyState> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, state: HotkeyState) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(state);
    }
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
            let mut started = false;
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
                    Event::Hotkey(state) => {
                        let settings = current(&app);
                        tray.show(*state, &texts, &settings.hotkey.to_string());
                        if let Some(status) = app.try_state::<HotkeyStatus>() {
                            status.set(*state);
                        }
                        update::hotkey_state(&app, *state);
                        let first = !std::mem::replace(&mut started, true);
                        if first && *state != HotkeyState::Registered && settings.ask_about_hotkey {
                            question::ask_about_hotkey(
                                &app,
                                &texts,
                                *state,
                                settings.hotkey,
                                &sender,
                            );
                        }
                    }
                    Event::PrintScreenYields => settings::change(&app, |current| Settings {
                        print_screen_yields: true,
                        ..current.clone()
                    }),
                    Event::HotkeyWhileRecording => update::hotkey_pressed(&app),
                    Event::StartWithWindows {
                        request, failure, ..
                    } => {
                        if app
                            .try_state::<StartupStatus>()
                            .is_some_and(|status| status.answered(*request, failure.clone()))
                        {
                            update::start_with_windows_problem(&app, failure.as_deref());
                        }
                    }
                    Event::Notice { notices, near }
                        if notice::worth_a_label(notices, current(&app).success_labels) =>
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

fn current(app: &AppHandle) -> Settings {
    app.try_state::<Store>()
        .map(|store| store.current())
        .unwrap_or_default()
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
        Event::Hotkey(state) => format!("hotkey {}", update::state_name(*state)),
        Event::PrintScreenYields => "print screen yields to Bediz; kept".to_owned(),
        Event::HotkeyWhileRecording => "hotkey pressed while a new one was asked for".to_owned(),
        Event::StartWithWindows { on, failure, .. } => {
            let wanted = if *on { "on" } else { "off" };
            match failure {
                Some(reason) => {
                    format!("start with Windows could not be turned {wanted}: {reason}")
                }
                None => format!("start with Windows {wanted}"),
            }
        }
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
