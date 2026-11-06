#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod i18n;
mod log;
mod question;
mod settings;
mod settings_window;
mod setup;
mod startup;
mod tray;
mod update;

fn main() {
    let Some(_first) = bediz_platform::first_instance() else {
        return;
    };
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::language,
            commands::settings,
            commands::set_settings,
            commands::settings_ready,
            commands::version,
            commands::open_logs,
            commands::open_source,
            commands::start_with_windows_problem,
            commands::hotkey_state,
            commands::record_hotkey,
            commands::take_print_screen,
            commands::retry_hotkey,
            commands::save_folder,
            commands::open_save_folder,
            commands::choose_save_folder,
        ])
        .setup(setup::setup)
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(setup::on_run_event),
        Err(error) => eprintln!("Bediz could not start: {error}"),
    }
}
