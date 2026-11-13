//! Starts the app's parts in order: the log, the settings, the tray, the platform and the events
//! thread, and ends them in order on exit.

use std::error::Error;
use std::sync::mpsc::{self, Sender};

use bediz_platform::{Command, Event, Platform, preferred_languages};
use tauri::{App, AppHandle, Manager, RunEvent};

use crate::events::{self, Events, HotkeyStatus, Message, Messages};
use crate::gif_window::GifState;
use crate::i18n::{Language, Texts};
use crate::log::Log;
use crate::settings::{self, Store};
use crate::settings_window::Section;
use crate::startup::{self, StartupStatus};
use crate::tray::Tray;
use crate::update;
use crate::updater::{self, Updates};

pub fn setup(app: &mut App) -> Result<(), Box<dyn Error>> {
    let languages = preferred_languages();
    let header = format!(
        "start version={} languages={}",
        env!("CARGO_PKG_VERSION"),
        languages.join(",")
    );
    let log = Log::open(app.path().app_local_data_dir()?.join("logs"), header)?;
    log.catch_panics();
    log.info("started");
    let (sender, receiver) = mpsc::channel();
    let store = start_store(app, &log, sender.clone())?;
    let settings = store.current();
    let texts = Texts::for_choice(settings.language.as_deref(), &languages);
    app.manage(store);
    app.manage(Language::new(texts.language()));
    app.manage(HotkeyStatus::default());
    app.manage(StartupStatus::default());
    app.manage(Updates::default());
    app.manage(Section::default());
    app.manage(GifState::default());
    app.manage(Messages(sender.clone()));
    let tray = Tray::create(
        app,
        &texts,
        log.directory().to_path_buf(),
        sender.clone(),
        settings.start_with_windows,
    )?;
    let platform_events = sender.clone();
    let handle = app.handle().clone();
    match Platform::start(move |event| match event {
        // Kept at once, not through the events thread, so a save finishing on quit still counts.
        Event::LastRegion(region) => settings::keep_last_region(&handle, region),
        event => {
            let _ = platform_events.send(Message::Platform(event));
        }
    }) {
        Ok(platform) => {
            platform.send(Command::SetMenuTexts(Box::new(texts.menu_texts())));
            platform.send(Command::SetLastRegion(settings.last_region));
            settings::send(&platform, &settings);
            // A development build would otherwise make itself start at every sign-in.
            if !cfg!(debug_assertions) {
                platform.send(Command::SetStartWithWindows {
                    on: settings.start_with_windows,
                    request: startup::AT_START,
                });
            }
            app.manage(platform);
        }
        Err(error) => log.warn(&format!("failed {error}")),
    }
    let events = Events { receiver, sender };
    events::spawn(events, log, tray, app.handle().clone(), languages)?;
    if settings.check_for_updates {
        updater::check(app.handle());
    }
    Ok(())
}

fn start_store(app: &App, log: &Log, notes: Sender<Message>) -> Result<Store, Box<dyn Error>> {
    let handle = app.handle().clone();
    let path = app.path().app_config_dir()?.join("settings.json");
    let (store, note) = Store::start(path, move |dropped, saved, reason| {
        settings::apply(&handle, dropped, saved);
        update::settings_not_saved(&handle, reason);
        let _ = notes.send(Message::Note(format!(
            "the settings could not be saved, so the saved ones are back: {reason}"
        )));
    })?;
    if let Some(note) = note {
        log.warn(&format!("note {note}"));
    }
    Ok(store)
}

pub fn send(app: &AppHandle, command: Command) {
    if let Some(platform) = app.try_state::<Platform>() {
        platform.send(command);
    }
}

pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Tauri exits without dropping managed state; saves end first, as one still changes a setting.
        RunEvent::Exit => {
            let platform = app.try_state::<Platform>();
            if let Some(platform) = &platform {
                platform.finish_saves();
            }
            if let Some(store) = app.try_state::<Store>() {
                store.shutdown();
            }
            if let Some(platform) = platform {
                platform.shutdown();
            }
        }
        _ => {}
    }
}
