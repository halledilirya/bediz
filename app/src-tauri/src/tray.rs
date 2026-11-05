use std::path::PathBuf;

use bediz_platform::{Command, HotkeyState, Platform};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::i18n::Texts;

const ID: &str = "main";
const TAKE_SCREENSHOT: &str = "tray-take-screenshot";
const RETRY: &str = "tray-retry";
const TAKE_PRINT_SCREEN: &str = "tray-take-print-screen";
const OPEN_LOGS: &str = "tray-open-logs";
const QUIT: &str = "tray-quit";
const STATUS_POSITION: usize = 2;

pub struct Tray {
    icon: TrayIcon<Wry>,
    menu: Menu<Wry>,
    status: MenuItem<Wry>,
    retry: MenuItem<Wry>,
    take_print_screen: MenuItem<Wry>,
    separator: PredefinedMenuItem<Wry>,
    texts: StateTexts,
}

struct StateTexts {
    tooltip: String,
    tooltip_taken: String,
    tooltip_snipping_tool: String,
    taken: String,
    snipping_tool: String,
}

impl Tray {
    pub fn create(app: &App, texts: &Texts, logs: PathBuf) -> tauri::Result<Self> {
        let item = |id: &str| MenuItem::with_id(app, id, texts.get(id), true, None::<&str>);
        let take = item(TAKE_SCREENSHOT)?;
        let retry = item(RETRY)?;
        let take_print_screen = item(TAKE_PRINT_SCREEN)?;
        let open_logs = item(OPEN_LOGS)?;
        let quit = item(QUIT)?;
        let status = MenuItem::new(app, "", false, None::<&str>)?;
        let separator = PredefinedMenuItem::separator(app)?;
        let menu = Menu::with_items(
            app,
            &[
                &take,
                &PredefinedMenuItem::separator(app)?,
                &open_logs,
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
                TAKE_SCREENSHOT => send(app, Command::CaptureAfterMenu),
                RETRY => send(app, Command::RetryHotkey),
                TAKE_PRINT_SCREEN => send(app, Command::TakePrintScreen),
                OPEN_LOGS => {
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
                } => send(icon.app_handle(), Command::Capture),
                TrayIconEvent::Enter { .. } => send(icon.app_handle(), Command::RetryHotkey),
                _ => {}
            })
            .build(app)?;
        Ok(Self {
            icon,
            menu,
            status,
            retry,
            take_print_screen,
            separator,
            texts: StateTexts {
                tooltip: texts.get("tray-tooltip"),
                tooltip_taken: texts.get("tray-tooltip-hotkey-taken"),
                tooltip_snipping_tool: texts.get("tray-tooltip-snipping-tool"),
                taken: texts.get("tray-hotkey-taken"),
                snipping_tool: texts.get("tray-hotkey-snipping-tool"),
            },
        })
    }

    pub fn show(&self, state: HotkeyState) {
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
            HotkeyState::Registered => (&self.texts.tooltip, None),
            HotkeyState::Taken => (
                &self.texts.tooltip_taken,
                Some((&self.texts.taken, &self.retry)),
            ),
            HotkeyState::SnippingTool => (
                &self.texts.tooltip_snipping_tool,
                Some((&self.texts.snipping_tool, &self.take_print_screen)),
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

fn send(app: &AppHandle, command: Command) {
    if let Some(platform) = app.try_state::<Platform>() {
        platform.send(command);
    }
}
