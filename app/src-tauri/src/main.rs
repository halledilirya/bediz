#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod events;
mod i18n;
mod log;
mod setup;
mod tray;

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(setup::setup)
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(setup::on_run_event),
        Err(error) => eprintln!("Bediz could not start: {error}"),
    }
}
