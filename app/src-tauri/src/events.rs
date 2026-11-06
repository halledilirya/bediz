use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, PoisonError};

use bediz_core::geometry::Rect;
use bediz_core::measurement::BUDGET;
use bediz_core::notice::{self, Notice};
use bediz_core::settings::Settings;
use bediz_platform::{Command, Event, HotkeyState, Label, Platform};
use tauri::{AppHandle, Manager};

use crate::i18n::{Language, Texts};
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
    Language,
    Note(String),
    Info(String),
}

pub struct Events {
    pub receiver: Receiver<Message>,
    pub sender: Sender<Message>,
}

pub struct Messages(pub Sender<Message>);

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

struct Dispatcher {
    app: AppHandle,
    log: Log,
    tray: Tray,
    texts: Texts,
    system_languages: Vec<String>,
    sender: Sender<Message>,
    started: bool,
}

pub fn spawn(
    events: Events,
    log: Log,
    tray: Tray,
    app: AppHandle,
    system_languages: Vec<String>,
) -> std::io::Result<()> {
    let Events { receiver, sender } = events;
    std::thread::Builder::new()
        .name("bediz-events".to_owned())
        .spawn(move || {
            let choice = current(&app).language;
            let mut dispatcher = Dispatcher {
                texts: Texts::for_choice(choice.as_deref(), &system_languages),
                app,
                log,
                tray,
                system_languages,
                sender,
                started: false,
            };
            for message in receiver {
                if catch_unwind(AssertUnwindSafe(|| dispatcher.handle(message))).is_err() {
                    dispatcher
                        .log
                        .warn("an internal error was caught while handling an event");
                }
            }
        })?;
    Ok(())
}

impl Dispatcher {
    fn handle(&mut self, message: Message) {
        match message {
            Message::Platform(event) => {
                self.on_event(&event);
                record(&self.log, &event);
            }
            Message::OpenSettings => self.open_settings(),
            Message::Language => self.change_language(),
            Message::Note(note) => self.log.warn(&format!("note {note}")),
            Message::Info(line) => self.log.info(&line),
        }
    }

    fn on_event(&mut self, event: &Event) {
        match event {
            Event::Hotkey(state) => self.on_hotkey(*state),
            Event::PrintScreenYields => settings::change(&self.app, |current| Settings {
                print_screen_yields: true,
                ..current.clone()
            }),
            Event::HotkeyWhileRecording => update::hotkey_pressed(&self.app),
            Event::StartWithWindows {
                request, failure, ..
            } => {
                if self
                    .app
                    .try_state::<StartupStatus>()
                    .is_some_and(|status| status.answered(*request, failure.clone()))
                {
                    update::start_with_windows_problem(&self.app, failure.as_deref());
                }
            }
            Event::Notice { notices, near }
                if notice::worth_a_label(notices, current(&self.app).success_labels) =>
            {
                self.show_label(notices, *near);
            }
            Event::SecondStart => self.open_settings(),
            _ => {}
        }
    }

    fn on_hotkey(&mut self, state: HotkeyState) {
        let settings = current(&self.app);
        self.tray
            .show(state, &self.texts, &settings.hotkey.to_string());
        if let Some(status) = self.app.try_state::<HotkeyStatus>() {
            status.set(state);
        }
        update::hotkey_state(&self.app, state);
        let first = !std::mem::replace(&mut self.started, true);
        if first && state != HotkeyState::Registered && settings.ask_about_hotkey {
            question::ask_about_hotkey(
                &self.app,
                &self.texts,
                state,
                settings.hotkey,
                &self.sender,
            );
        }
    }

    fn open_settings(&self) {
        let title = self.texts.get("settings-title");
        if let Err(error) = settings_window::open(&self.app, &title, &self.sender) {
            self.log
                .warn(&format!("failed to open the settings: {error}"));
        }
    }

    fn change_language(&mut self) {
        let choice = current(&self.app).language;
        self.texts = Texts::for_choice(choice.as_deref(), &self.system_languages);
        if let Some(language) = self.app.try_state::<Language>() {
            language.set(self.texts.language());
        }
        self.tray.retext(&self.texts);
        if let Some(state) = self
            .app
            .try_state::<HotkeyStatus>()
            .and_then(|status| status.get())
        {
            let key = current(&self.app).hotkey.to_string();
            self.tray.show(state, &self.texts, &key);
        }
        if let Some(platform) = self.app.try_state::<Platform>() {
            platform.send(Command::SetMenuTexts(self.texts.menu_texts()));
        }
        settings_window::retitle(&self.app, &self.texts.get("settings-title"));
        update::language_changed(&self.app);
    }

    fn show_label(&self, notices: &[Notice], near: Rect) {
        let Some(platform) = self.app.try_state::<Platform>() else {
            return;
        };
        platform.send(Command::ShowLabel(Label {
            lines: notices
                .iter()
                .map(|notice| self.texts.notice(notice))
                .collect(),
            near,
            warning: notices.iter().any(Notice::is_failure),
        }));
    }
}

fn current(app: &AppHandle) -> Settings {
    app.try_state::<Store>()
        .map(|store| store.current())
        .unwrap_or_default()
}

fn record(log: &Log, event: &Event) {
    match event {
        Event::Hotkey(state) => {
            let line = format!("hotkey {}", update::state_name(*state));
            if *state == HotkeyState::Registered {
                log.info(&line);
            } else {
                log.warn(&line);
            }
        }
        Event::PrintScreenYields => log.info("print screen yields to Bediz; kept"),
        Event::HotkeyWhileRecording => log.info("hotkey pressed while a new one was asked for"),
        Event::LastRegion(_) => {}
        Event::StartWithWindows { on, failure, .. } => {
            let wanted = if *on { "on" } else { "off" };
            match failure {
                Some(reason) => log.warn(&format!(
                    "start with Windows could not be turned {wanted}: {reason}"
                )),
                None => log.info(&format!("start with Windows {wanted}")),
            }
        }
        Event::Idle(memory) => log.info(&format!("idle {}", memory.log_fields())),
        Event::Measured(measurement) if measurement.over_budget() => log.warn(&format!(
            "{} over the {} ms budget",
            measurement.log_line(),
            BUDGET.as_millis()
        )),
        Event::Measured(measurement) => log.info(&measurement.log_line()),
        Event::Notice { notices, near } => {
            let said: Vec<String> = notices.iter().map(Notice::to_string).collect();
            let line = if near.width() > 1 || near.height() > 1 {
                format!("{} {}x{}", said.join("; "), near.width(), near.height())
            } else {
                said.join("; ")
            };
            if notices.iter().any(Notice::is_failure) {
                log.warn(&line);
            } else {
                log.info(&line);
            }
        }
        Event::Note(note) => log.warn(&format!("note {note}")),
        Event::Failed(failure) => log.warn(&format!("failed {failure}")),
        Event::SecondStart => log.info("second start"),
    }
}
