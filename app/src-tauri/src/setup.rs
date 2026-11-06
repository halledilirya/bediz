use std::error::Error;
use std::sync::mpsc::{self, Sender};

use bediz_platform::{Command, MenuTexts, Platform, preferred_languages};
use tauri::{App, AppHandle, Manager, RunEvent};

use crate::events::{self, Events, HotkeyStatus, Message};
use crate::i18n::{Language, Texts};
use crate::log::Log;
use crate::settings::{self, Store};
use crate::startup::{self, StartupStatus};
use crate::tray::Tray;
use crate::update;

pub fn setup(app: &mut App) -> Result<(), Box<dyn Error>> {
    let languages = preferred_languages();
    let texts = Texts::for_languages(&languages);
    let log = Log::open(app.path().app_local_data_dir()?.join("logs"))?;
    log.write(&format!(
        "start version={} languages={}",
        env!("CARGO_PKG_VERSION"),
        languages.join(",")
    ));
    let (sender, receiver) = mpsc::channel();
    let store = start_store(app, &log, sender.clone())?;
    let settings = store.current();
    app.manage(store);
    app.manage(Language(texts.language()));
    app.manage(HotkeyStatus::default());
    app.manage(StartupStatus::default());
    let tray = Tray::create(
        app,
        &texts,
        log.directory().to_path_buf(),
        sender.clone(),
        settings.start_with_windows,
    )?;
    let platform_events = sender.clone();
    match Platform::start(move |event| {
        let _ = platform_events.send(Message::Platform(event));
    }) {
        Ok(platform) => {
            platform.send(Command::SetMenuTexts(menu_texts(&texts)));
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
        Err(error) => log.write(&format!("failed {error}")),
    }
    let events = Events { receiver, sender };
    events::spawn(events, log, tray, app.handle().clone(), languages)?;
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
        log.write(&format!("note {note}"));
    }
    Ok(store)
}

fn menu_texts(texts: &Texts) -> MenuTexts {
    MenuTexts {
        copy_and_save: texts.get("overlay-menu-copy-and-save"),
        copy: texts.get("overlay-menu-copy"),
        save: texts.get("overlay-menu-save"),
        save_as: texts.get("overlay-menu-save-as"),
        copy_colour: texts.get("overlay-menu-copy-colour"),
        whole_monitor: texts.get("overlay-menu-whole-monitor"),
        last_region: texts.get("overlay-menu-last-region"),
        cancel: texts.get("overlay-menu-cancel"),
    }
}

pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Tauri ends the process without dropping managed state, so queued writes finish here.
        RunEvent::Exit => {
            if let Some(store) = app.try_state::<Store>() {
                store.shutdown();
            }
            if let Some(platform) = app.try_state::<Platform>() {
                platform.shutdown();
            }
        }
        _ => {}
    }
}
