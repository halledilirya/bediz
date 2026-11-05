use std::error::Error;
use std::sync::mpsc;

use bediz_platform::{Platform, preferred_languages};
use tauri::{App, Manager};

use crate::events;
use crate::i18n::Texts;
use crate::log::Log;
use crate::tray::Tray;

pub fn setup(app: &mut App) -> Result<(), Box<dyn Error>> {
    let languages = preferred_languages();
    let texts = Texts::for_languages(&languages);
    let log = Log::open(app.path().local_data_dir()?.join("Bediz").join("logs"))?;
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
    events::spawn(receiver, log, tray)?;
    Ok(())
}
