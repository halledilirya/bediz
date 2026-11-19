//! The tray icon and its menu: capturing now or in a few seconds, recording controls, opening an
//! image to mark up, hotkey problems, an update found, the settings, help and quit.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use bediz_platform::{Command, HotkeyState, ImageSource, RecordingAction, RecordingState};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::events::Message;
use crate::i18n::Texts;
use crate::setup;
use crate::updater;

const ID: &str = "main";
const TAKE_SCREENSHOT: &str = "tray-take-screenshot";
const DELAYED: &str = "tray-delayed-screenshot";
pub const DELAYS: [(&str, u64); 3] = [
    ("tray-delay-three", 3),
    ("tray-delay-five", 5),
    ("tray-delay-ten", 10),
];
const CANCEL_DELAY: &str = "tray-cancel-delay";
const RECORD: &str = "tray-record";
const RECORD_GIF: &str = "tray-record-gif";
const PAUSE: &str = "recording-pause";
const RESUME: &str = "recording-resume";
const FINISH: &str = "recording-finish";
const RESTART: &str = "recording-restart";
const DISCARD: &str = "recording-discard";
const OPEN_IMAGE: &str = "tray-open-image";
const OPEN_IMAGE_CLIPBOARD: &str = "tray-open-image-clipboard";
const OPEN_IMAGE_FILE: &str = "tray-open-image-file";
const RETRY: &str = "tray-retry";
const TAKE_PRINT_SCREEN: &str = "tray-take-print-screen";
const UPDATE_TO: &str = "tray-update-to";
const SETTINGS: &str = "tray-settings";
const HELP: &str = "tray-help";
const CHECK_UPDATES: &str = "tray-check-updates";
const OPEN_LOGS: &str = "tray-open-logs";
const ABOUT: &str = "tray-about";
const QUIT: &str = "tray-quit";
const DELAY_POSITION: usize = 1;
const STATUS_POSITION: usize = 5;

pub struct Tray {
    icon: TrayIcon<Wry>,
    menu: Menu<Wry>,
    status: MenuItem<Wry>,
    take: MenuItem<Wry>,
    delayed: Submenu<Wry>,
    delays: Vec<MenuItem<Wry>>,
    cancel_delay: MenuItem<Wry>,
    record: Submenu<Wry>,
    record_gif: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    resume: MenuItem<Wry>,
    finish: MenuItem<Wry>,
    restart: MenuItem<Wry>,
    discard: MenuItem<Wry>,
    open_image: Submenu<Wry>,
    open_image_clipboard: MenuItem<Wry>,
    open_image_file: MenuItem<Wry>,
    retry: MenuItem<Wry>,
    take_print_screen: MenuItem<Wry>,
    update_to: MenuItem<Wry>,
    settings: MenuItem<Wry>,
    help: Submenu<Wry>,
    updates: MenuItem<Wry>,
    open_logs: MenuItem<Wry>,
    about: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    separator: PredefinedMenuItem<Wry>,
}

impl Tray {
    pub fn create(
        app: &App,
        texts: &Texts,
        logs: PathBuf,
        messages: Sender<Message>,
    ) -> tauri::Result<Self> {
        let item = |id: &str| MenuItem::with_id(app, id, texts.get(id), true, None::<&str>);
        let take = item(TAKE_SCREENSHOT)?;
        let delays = DELAYS
            .iter()
            .map(|(id, _)| item(id))
            .collect::<tauri::Result<Vec<_>>>()?;
        let delay_items: Vec<&dyn IsMenuItem<Wry>> = delays
            .iter()
            .map(|delay| delay as &dyn IsMenuItem<Wry>)
            .collect();
        let delayed =
            Submenu::with_id_and_items(app, DELAYED, texts.get(DELAYED), true, &delay_items)?;
        let cancel_delay = item(CANCEL_DELAY)?;
        let record_gif = item(RECORD_GIF)?;
        let pause = item(PAUSE)?;
        let resume = item(RESUME)?;
        let finish = item(FINISH)?;
        let restart = item(RESTART)?;
        let discard = item(DISCARD)?;
        let record =
            Submenu::with_id_and_items(app, RECORD, texts.get(RECORD), true, &[&record_gif])?;
        let open_image_clipboard = item(OPEN_IMAGE_CLIPBOARD)?;
        let open_image_file = item(OPEN_IMAGE_FILE)?;
        let open_image = Submenu::with_id_and_items(
            app,
            OPEN_IMAGE,
            texts.get(OPEN_IMAGE),
            true,
            &[&open_image_clipboard, &open_image_file],
        )?;
        let retry = item(RETRY)?;
        let take_print_screen = item(TAKE_PRINT_SCREEN)?;
        let update_to = MenuItem::with_id(app, UPDATE_TO, "", true, None::<&str>)?;
        let settings = item(SETTINGS)?;
        let updates = item(CHECK_UPDATES)?;
        let open_logs = item(OPEN_LOGS)?;
        let about = item(ABOUT)?;
        let mut help_items: Vec<&dyn IsMenuItem<Wry>> = Vec::new();
        if updater::offered() {
            help_items.push(&updates);
        }
        help_items.extend([&open_logs as &dyn IsMenuItem<Wry>, &about]);
        let help = Submenu::with_id_and_items(app, HELP, texts.get(HELP), true, &help_items)?;
        let quit = item(QUIT)?;
        let status = MenuItem::new(app, "", false, None::<&str>)?;
        let separator = PredefinedMenuItem::separator(app)?;
        let below_take = PredefinedMenuItem::separator(app)?;
        let menu = Menu::with_items(
            app,
            &[
                &take,
                &delayed,
                &record,
                &open_image,
                &below_take,
                &settings,
                &help,
                &quit,
            ],
        )?;
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
                CANCEL_DELAY => setup::send(app, Command::CancelDelay),
                RECORD_GIF => setup::send(app, Command::RecordGif),
                PAUSE => setup::send(app, Command::Recording(RecordingAction::Pause)),
                RESUME => setup::send(app, Command::Recording(RecordingAction::Resume)),
                FINISH => setup::send(app, Command::Recording(RecordingAction::Finish)),
                RESTART => setup::send(app, Command::Recording(RecordingAction::Restart)),
                DISCARD => setup::send(app, Command::Recording(RecordingAction::Discard)),
                OPEN_IMAGE_CLIPBOARD => setup::send(app, Command::MarkUp(ImageSource::Clipboard)),
                OPEN_IMAGE_FILE => setup::send(app, Command::MarkUp(ImageSource::File)),
                RETRY => setup::send(app, Command::RetryHotkey),
                TAKE_PRINT_SCREEN => setup::send(app, Command::TakePrintScreen),
                SETTINGS => {
                    let _ = messages.send(Message::OpenSettings);
                }
                UPDATE_TO | CHECK_UPDATES => {
                    let _ = messages.send(Message::ShowUpdates);
                }
                OPEN_LOGS => {
                    let logs = bediz_platform::seen_outside(&logs);
                    let _ = app.opener().open_path(logs.to_string_lossy(), None::<&str>);
                }
                ABOUT => {
                    let _ = messages.send(Message::ShowAbout);
                }
                QUIT => {
                    let _ = messages.send(Message::Quit);
                }
                other => {
                    if let Some((_, seconds)) = DELAYS.iter().find(|(delay, _)| *delay == other) {
                        setup::send(app, Command::CaptureIn(Duration::from_secs(*seconds)));
                    }
                }
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
            delayed,
            delays,
            cancel_delay,
            record,
            record_gif,
            pause,
            resume,
            finish,
            restart,
            discard,
            open_image,
            open_image_clipboard,
            open_image_file,
            retry,
            take_print_screen,
            update_to,
            settings,
            help,
            updates,
            open_logs,
            about,
            quit,
            separator,
        })
    }

    pub fn retext(&self, texts: &Texts) {
        let items = [
            (&self.take, TAKE_SCREENSHOT),
            (&self.cancel_delay, CANCEL_DELAY),
            (&self.record_gif, RECORD_GIF),
            (&self.pause, PAUSE),
            (&self.resume, RESUME),
            (&self.finish, FINISH),
            (&self.restart, RESTART),
            (&self.discard, DISCARD),
            (&self.open_image_clipboard, OPEN_IMAGE_CLIPBOARD),
            (&self.open_image_file, OPEN_IMAGE_FILE),
            (&self.retry, RETRY),
            (&self.take_print_screen, TAKE_PRINT_SCREEN),
            (&self.settings, SETTINGS),
            (&self.updates, CHECK_UPDATES),
            (&self.open_logs, OPEN_LOGS),
            (&self.about, ABOUT),
            (&self.quit, QUIT),
        ];
        for (item, id) in items {
            let _ = item.set_text(texts.get(id));
        }
        for (item, (id, _)) in self.delays.iter().zip(DELAYS) {
            let _ = item.set_text(texts.get(id));
        }
        let submenus = [
            (&self.delayed, DELAYED),
            (&self.record, RECORD),
            (&self.open_image, OPEN_IMAGE),
            (&self.help, HELP),
        ];
        for (submenu, id) in submenus {
            let _ = submenu.set_text(texts.get(id));
        }
        let _ = self.icon.set_tooltip(Some(texts.get("tray-tooltip")));
    }

    pub fn show_update(&self, texts: &Texts, available: Option<&str>) {
        let _ = self.menu.remove(&self.update_to);
        let Some(version) = available else {
            return;
        };
        let _ = self
            .update_to
            .set_text(texts.get_with(UPDATE_TO, "version", version));
        let settings = self
            .menu
            .items()
            .ok()
            .and_then(|items| items.iter().position(|item| *item.id() == SETTINGS));
        if let Some(position) = settings {
            let _ = self.menu.insert(&self.update_to, position);
        }
    }

    pub fn show_recording(&self, state: RecordingState) {
        let every: [&dyn IsMenuItem<Wry>; 6] = [
            &self.record_gif,
            &self.pause,
            &self.resume,
            &self.finish,
            &self.restart,
            &self.discard,
        ];
        for item in every {
            let _ = self.record.remove(item);
        }
        let _ = self.take.set_enabled(state == RecordingState::Off);
        let _ = self.delayed.set_enabled(state == RecordingState::Off);
        let shown: [&dyn IsMenuItem<Wry>; 4] = match state {
            RecordingState::Off => {
                let _ = self.record.append(&self.record_gif);
                return;
            }
            RecordingState::Going => [&self.pause, &self.finish, &self.restart, &self.discard],
            RecordingState::Paused => [&self.resume, &self.finish, &self.restart, &self.discard],
        };
        let _ = self.record.append_items(&shown);
    }

    pub fn show_delay(&self, waiting: bool) {
        let _ = self.menu.remove(&self.delayed);
        let _ = self.menu.remove(&self.cancel_delay);
        let shown: &dyn IsMenuItem<Wry> = if waiting {
            &self.cancel_delay
        } else {
            &self.delayed
        };
        let _ = self.menu.insert(shown, DELAY_POSITION);
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
