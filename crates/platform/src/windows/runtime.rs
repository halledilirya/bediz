mod delivery;
mod displays;
mod save_as;
mod screenshot;

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bediz_core::capture::Allocation;
use bediz_core::delivery::CopyOrder;
use windows::Win32::Foundation::{HINSTANCE, HWND, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, HWND_MESSAGE, KillTimer, MSG,
    MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    SetTimer, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_HOTKEY, WM_KEYDOWN,
    WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_QUIT, WM_RBUTTONUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WaitMessage,
};
use windows::core::{Result, w};

use bediz_core::geometry::Rect;

use super::clock::{self, Clock};
use super::desktop_switch::{self, DesktopSwitchHook};
use super::hotkey::{self, Change, PrintScreen};
use super::label_window::LabelWindow;
use super::save_dialog::Answers;
use super::scene::Scene;
use super::session::Session;
use super::virtual_desktop::VirtualDesktops;
use super::window_class::{self, MESSAGE_CLASS};
use super::window_list::WindowList;
use super::worker::Worker;
use crate::event::{Command, Event, Label, MenuTexts};
use delivery::{PendingCopy, Tracked};
use save_as::{PendingSaveAs, Retry};

pub const WM_COMMANDS: u32 = WM_APP + 1;
pub const WM_STOP: u32 = WM_APP + 2;
pub const WM_OUTCOMES: u32 = WM_APP + 3;
pub const WM_SAVE_AS: u32 = WM_APP + 4;

const MENU_FADE_TIMER: usize = 1;
const DISPLAY_TIMER: usize = 2;
const CLIPBOARD_TIMER: usize = 3;
const LABEL_TIMER: usize = 4;
const DISPLAY_SETTLE_MILLISECONDS: u32 = 500;
const LABEL_MILLISECONDS: u32 = 1500;
const WARNING_MILLISECONDS: u32 = 6000;

pub type Commands = Arc<Mutex<Vec<Command>>>;
pub type Sink = Box<dyn Fn(Event) + Send>;

struct MessageWindow(HWND);

impl Drop for MessageWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

struct Runtime {
    session: Option<Session>,
    scene: Option<Scene>,
    displays_changed: bool,
    menu_capture: bool,
    print_screen: PrintScreen,
    desktop_switch: Option<DesktopSwitchHook>,
    desktops: Option<VirtualDesktops>,
    worker: Option<Worker>,
    copies: CopyOrder,
    clipboard: Option<PendingCopy>,
    deliveries: u64,
    reports: Vec<Tracked>,
    label: Option<LabelWindow>,
    menu_texts: Option<MenuTexts>,
    save_as: Option<PendingSaveAs>,
    save_as_answers: Answers,
    retries: VecDeque<Retry>,
    window: MessageWindow,
    instance: HINSTANCE,
    commands: Commands,
    sink: Sink,
    clock: Clock,
    allocation: Allocation,
    windows: WindowList,
    detect_windows: bool,
    last_region: Option<Rect>,
}

pub fn run(commands: Commands, sink: Sink, ready: Sender<std::result::Result<isize, String>>) {
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        let _ = ready.send(Err(error.to_string()));
        return;
    }
    match Runtime::create(commands, sink) {
        Ok(mut runtime) => {
            let _ = ready.send(Ok(runtime.window.0.0 as isize));
            runtime.start();
            runtime.pump();
            runtime.stop();
        }
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
        }
    }
    unsafe { CoUninitialize() };
}

impl Runtime {
    fn create(commands: Commands, sink: Sink) -> Result<Self> {
        let instance = HINSTANCE::from(unsafe { GetModuleHandleW(None) }?);
        window_class::register(instance)?;
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                MESSAGE_CLASS,
                w!("Bediz"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
        }?;
        Ok(Self {
            session: None,
            scene: None,
            displays_changed: false,
            menu_capture: false,
            print_screen: PrintScreen::new(window),
            desktop_switch: DesktopSwitchHook::install(),
            desktops: None,
            worker: None,
            copies: CopyOrder::default(),
            clipboard: None,
            deliveries: 0,
            reports: Vec::new(),
            label: None,
            menu_texts: None,
            save_as: None,
            save_as_answers: Answers::default(),
            retries: VecDeque::new(),
            window: MessageWindow(window),
            instance,
            commands,
            sink,
            clock: Clock::new(),
            allocation: Allocation::default(),
            windows: WindowList::new(),
            detect_windows: false,
            last_region: None,
        })
    }

    fn start(&mut self) {
        let mut notes = Vec::new();
        self.print_screen.start_watching(&mut notes);
        if self.desktop_switch.is_none() {
            notes.push(
                "desktop switches are not watched; capture recovers on the next hotkey".to_owned(),
            );
        }
        match VirtualDesktops::new() {
            Ok(desktops) => self.desktops = Some(desktops),
            Err(error) => notes.push(format!(
                "virtual desktops are not followed; the overlay stays where it was made: {error}"
            )),
        }
        match Worker::start(self.window.0) {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => notes.push(format!(
                "the thread that encodes and saves could not start: {error}"
            )),
        }
        match LabelWindow::create(self.instance) {
            Ok(label) => self.label = Some(label),
            Err(error) => notes.push(format!(
                "confirmations cannot be shown; they still go to the log: {error}"
            )),
        }
        self.emit_notes(notes);
        let change = self.print_screen.refresh();
        self.report_hotkey(change);
        self.build_scene();
        let overlay = self
            .scene
            .as_ref()
            .and_then(|scene| scene.windows().first().copied());
        if let (Some(desktops), Some(overlay)) = (&self.desktops, overlay) {
            desktops.warm_up(overlay);
        }
        self.report_idle();
    }

    fn stop(&mut self) {
        self.hide();
        self.print_screen.unregister();
        self.worker = None;
        self.scene = None;
    }

    fn pump(&mut self) {
        loop {
            let event = self.print_screen.wake_event();
            let waited = unsafe {
                MsgWaitForMultipleObjectsEx(
                    Some(event.as_slice()),
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                )
            };
            match waited {
                WAIT_OBJECT_0 if event.is_some() => {
                    let mut notes = Vec::new();
                    let change = self.print_screen.setting_changed(&mut notes);
                    self.emit_notes(notes);
                    self.report_hotkey(change);
                }
                WAIT_FAILED if event.is_some() => {
                    self.print_screen.stop_watching();
                    self.emit_notes(vec![
                        "waiting on the Print Screen setting failed; it is no longer watched"
                            .to_owned(),
                    ]);
                }
                WAIT_FAILED => {
                    let _ = unsafe { WaitMessage() };
                }
                _ => {}
            }
            if !self.drain() {
                break;
            }
        }
    }

    fn drain(&mut self) -> bool {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == WM_QUIT || !self.dispatch(&message) || !self.after_message() {
                return false;
            }
        }
        // Sent messages and WinEvents run inside PeekMessageW, which then returns no message.
        self.after_message()
    }

    fn dispatch(&mut self, message: &MSG) -> bool {
        let own = message.hwnd == self.window.0;
        match message.message {
            WM_STOP if own => return false,
            WM_HOTKEY if own && message.wParam.0 == hotkey::PRINT_SCREEN as usize => {
                self.open(clock::message_age(message.time));
                let change = self.print_screen.arrived();
                self.report_hotkey(change);
            }
            WM_TIMER if own && message.wParam.0 == MENU_FADE_TIMER => {
                if self.menu_capture {
                    self.open(Duration::ZERO);
                }
            }
            WM_TIMER if own && message.wParam.0 == DISPLAY_TIMER => {
                if self.displays_changed {
                    self.rebuild_scene();
                }
            }
            WM_TIMER if own && message.wParam.0 == CLIPBOARD_TIMER => self.retry_clipboard(),
            WM_TIMER if own && message.wParam.0 == LABEL_TIMER => self.hide_label(),
            WM_COMMANDS if own => self.on_commands(),
            WM_OUTCOMES if own => self.on_outcomes(),
            WM_SAVE_AS if own => self.on_save_as_answer(),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_RBUTTONUP | WM_MOUSEMOVE
            | WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
                if self.is_overlay(message.hwnd) =>
            {
                self.on_input(message);
            }
            _ => unsafe {
                let _ = TranslateMessage(message);
                DispatchMessageW(message);
            },
        }
        true
    }

    fn is_overlay(&self, window: HWND) -> bool {
        self.scene.as_ref().is_some_and(|scene| scene.owns(window))
    }

    fn on_commands(&mut self) {
        let commands =
            std::mem::take(&mut *self.commands.lock().unwrap_or_else(PoisonError::into_inner));
        for command in commands {
            match command {
                Command::Capture => self.open(Duration::ZERO),
                Command::CaptureAfterMenu => self.open_after_menu(),
                Command::RetryHotkey => {
                    let change = self.print_screen.refresh();
                    self.report_hotkey(change);
                }
                Command::TakePrintScreen => {
                    let mut notes = Vec::new();
                    let change = self.print_screen.take_from_snipping_tool(&mut notes);
                    self.emit_notes(notes);
                    self.report_hotkey(change);
                }
                Command::SetAllocation(allocation) => self.set_allocation(allocation),
                Command::ShowLabel(label) => self.show_label(&label),
                Command::SetMenuTexts(texts) => self.menu_texts = Some(texts),
            }
        }
    }

    fn show_label(&mut self, label: &Label) {
        let Some(window) = &self.label else {
            return;
        };
        if let Err(error) = window.show(label) {
            self.emit_notes(vec![format!(
                "the confirmation could not be shown: {error}"
            )]);
            return;
        }
        let shown_for = if label.warning {
            WARNING_MILLISECONDS
        } else {
            LABEL_MILLISECONDS
        };
        let _ = unsafe { SetTimer(Some(self.window.0), LABEL_TIMER, shown_for, None) };
    }

    fn hide_label(&mut self) {
        let _ = unsafe { KillTimer(Some(self.window.0), LABEL_TIMER) };
        if let Some(window) = &self.label {
            window.hide();
        }
    }

    fn after_message(&mut self) -> bool {
        if window_class::take_close_requested() {
            self.hide();
        }
        if window_class::take_display_changed() {
            self.on_display_change();
        }
        if desktop_switch::take_switched() {
            self.hide();
            if let Some(scene) = self.scene.as_mut() {
                scene.reopen_capture();
            }
            let change = self.print_screen.refresh();
            self.report_hotkey(change);
        }
        let own = self.window.0;
        window_class::take_missed().into_iter().all(|message| {
            self.dispatch(&MSG {
                hwnd: own,
                message,
                ..Default::default()
            })
        })
    }

    fn report_hotkey(&self, change: Option<Change>) {
        if let Some(change) = change {
            (self.sink)(Event::Note(change.note));
            (self.sink)(Event::Hotkey(change.state));
        }
    }

    fn emit_notes(&self, notes: Vec<String>) {
        for note in notes {
            (self.sink)(Event::Note(note));
        }
    }

    fn report_idle(&self) {
        if let Some(scene) = &self.scene {
            (self.sink)(Event::Idle(scene.memory()));
        }
    }
}
