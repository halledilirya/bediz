use std::path::PathBuf;
use std::sync::mpsc::Sender;

use bediz_core::settings::Settings;
use bediz_platform::{Command, HotkeyState};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::events::Message;
use crate::i18n::Texts;
use crate::settings;
use crate::setup;
use crate::updater;

const ID: &str = "main";
const TAKE_SCREENSHOT: &str = "tray-take-screenshot";
const RETRY: &str = "tray-retry";
const TAKE_PRINT_SCREEN: &str = "tray-take-print-screen";
const START_WITH_WINDOWS: &str = "tray-start-with-windows";
const SETTINGS: &str = "tray-settings";
const CHECK_UPDATES: &str = "tray-check-updates";
const UPDATE_TO: &str = "tray-update-to";
const OPEN_LOGS: &str = "tray-open-logs";
const QUIT: &str = "tray-quit";
const STATUS_POSITION: usize = 2;

pub struct Tray {
    icon: TrayIcon<Wry>,
    menu: Menu<Wry>,
    status: MenuItem<Wry>,
    take: MenuItem<Wry>,
    retry: MenuItem<Wry>,
    take_print_screen: MenuItem<Wry>,
    startup: CheckMenuItem<Wry>,
    settings: MenuItem<Wry>,
    updates: MenuItem<Wry>,
    open_logs: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    separator: PredefinedMenuItem<Wry>,
}

pub struct StartupItem(CheckMenuItem<Wry>);

impl Tray {
    pub fn create(
        app: &App,
        texts: &Texts,
        logs: PathBuf,
        messages: Sender<Message>,
        start_with_windows: bool,
    ) -> tauri::Result<Self> {
        let item = |id: &str| MenuItem::with_id(app, id, texts.get(id), true, None::<&str>);
        let take = item(TAKE_SCREENSHOT)?;
        let retry = item(RETRY)?;
        let take_print_screen = item(TAKE_PRINT_SCREEN)?;
        let startup = CheckMenuItem::with_id(
            app,
            START_WITH_WINDOWS,
            texts.get(START_WITH_WINDOWS),
            true,
            start_with_windows,
            None::<&str>,
        )?;
        let settings = item(SETTINGS)?;
        let updates = item(CHECK_UPDATES)?;
        let open_logs = item(OPEN_LOGS)?;
        let quit = item(QUIT)?;
        let status = MenuItem::new(app, "", false, None::<&str>)?;
        let separator = PredefinedMenuItem::separator(app)?;
        let below_take = PredefinedMenuItem::separator(app)?;
        let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&take, &below_take, &startup, &settings];
        if updater::offered() {
            items.push(&updates);
        }
        items.extend([&open_logs as &dyn IsMenuItem<Wry>, &quit]);
        let menu = Menu::with_items(app, &items)?;
        app.manage(StartupItem(startup.clone()));
        let mut builder = TrayIconBuilder::with_id(ID)
            .menu(&menu)
            .show_menu_on_left_click(false)
            .tooltip(texts.get("tray-tooltip"));
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        let icon = builder
            .on_menu_event(move |app, event| match event.id().as_ref() {
                TAKE_SCREENSHOT => setup::send(app, Command::CaptureAfterMenu),
                RETRY => setup::send(app, Command::RetryHotkey),
                TAKE_PRINT_SCREEN => setup::send(app, Command::TakePrintScreen),
                START_WITH_WINDOWS => settings::change(app, |current| Settings {
                    start_with_windows: !current.start_with_windows,
                    ..current.clone()
                }),
                SETTINGS => {
                    let _ = messages.send(Message::OpenSettings);
                }
                CHECK_UPDATES => {
                    let _ = messages.send(Message::ShowUpdates);
                }
                OPEN_LOGS => {
                    let logs = bediz_platform::seen_outside(&logs);
                    let _ = app.opener().open_path(logs.to_string_lossy(), None::<&str>);
                }
                QUIT => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|icon, event| match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => setup::send(icon.app_handle(), Command::Capture),
                TrayIconEvent::Enter { .. } => setup::send(icon.app_handle(), Command::RetryHotkey),
                _ => {}
            })
            .build(app)?;
        Ok(Self {
            icon,
            menu,
            status,
            take,
            retry,
            take_print_screen,
            startup,
            settings,
            updates,
            open_logs,
            quit,
            separator,
        })
    }

    pub fn retext(&self, texts: &Texts) {
        let items = [
            (&self.take, TAKE_SCREENSHOT),
            (&self.retry, RETRY),
            (&self.take_print_screen, TAKE_PRINT_SCREEN),
            (&self.settings, SETTINGS),
            (&self.open_logs, OPEN_LOGS),
            (&self.quit, QUIT),
        ];
        for (item, id) in items {
            let _ = item.set_text(texts.get(id));
        }
        let _ = self.startup.set_text(texts.get(START_WITH_WINDOWS));
        let _ = self.icon.set_tooltip(Some(texts.get("tray-tooltip")));
    }

    pub fn show_update(&self, texts: &Texts, available: Option<&str>) {
        let text = match available {
            Some(version) => texts.get_with(UPDATE_TO, "version", version),
            None => texts.get(CHECK_UPDATES),
        };
        let _ = self.updates.set_text(text);
    }

    pub fn show(&self, state: HotkeyState, texts: &Texts, key: &str) {
        let optional: [&dyn IsMenuItem<Wry>; 4] = [
            &self.status,
            &self.retry,
            &self.take_print_screen,
            &self.separator,
        ];
        for item in optional {
            let _ = self.menu.remove(item);
        }
        let (tooltip, problem) = match state {
            HotkeyState::Registered => (texts.get("tray-tooltip"), None),
            HotkeyState::Taken => (
                texts.get_with("tray-tooltip-hotkey-taken", "key", key),
                Some((texts.get_with("tray-hotkey-taken", "key", key), &self.retry)),
            ),
            HotkeyState::SnippingTool => (
                texts.get("tray-tooltip-snipping-tool"),
                Some((
                    texts.get("tray-hotkey-snipping-tool"),
                    &self.take_print_screen,
                )),
            ),
        };
        let _ = self.icon.set_tooltip(Some(tooltip));
        if let Some((status, action)) = problem {
            let _ = self.status.set_text(status);
            let _ = self
                .menu
                .insert_items(&[&self.status, action, &self.separator], STATUS_POSITION);
        }
    }
}

pub fn show_icon(app: &AppHandle, visible: bool) {
    if let Some(icon) = app.tray_by_id(ID) {
        let _ = icon.set_visible(visible);
    }
}

// Waiting for the main thread here could deadlock with a command that waits for the settings.
pub fn show_start_with_windows(app: &AppHandle, on: bool) {
    if let Some(startup) = app.try_state::<StartupItem>() {
        let item = startup.0.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = item.set_checked(on);
        });
    }
}
