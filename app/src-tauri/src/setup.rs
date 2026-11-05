use std::error::Error;
use std::sync::mpsc;

use bediz_platform::{Platform, preferred_languages};
use tauri::{App, AppHandle, Manager, RunEvent};

use crate::events;
use crate::i18n::Texts;
use crate::log::Log;
use crate::tray::Tray;

pub fn setup(app: &mut App) -> Result<(), Box<dyn Error>> {
    let languages = preferred_languages();
    let texts = Texts::for_languages(&languages);
    let log = Log::open(app.path().app_local_data_dir()?.join("logs"))?;
    log.write(&format!(
        "start version={} languages={}",
        env!("CARGO_PKG_VERSION"),
        languages.join(",")
    ));
    let tray = Tray::create(app, &texts, log.directory().to_path_buf())?;
    let (sender, receiver) = mpsc::channel();
    match Platform::start(move |event| {
        let _ = sender.send(event);
    }) {
        Ok(platform) => {
            app.manage(platform);
        }
        Err(error) => log.write(&format!("failed {error}")),
    }
    events::spawn(receiver, log, tray, app.handle().clone(), languages)?;
    Ok(())
}

pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        // Tauri ends the process without dropping managed state, so the queued saves finish here.
        RunEvent::Exit => {
            if let Some(platform) = app.try_state::<Platform>() {
                platform.shutdown();
            }
        }
        _ => {}
    }
}
