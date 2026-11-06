mod commands;
mod delivery;
mod displays;
mod save_as;
mod screenshot;

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bediz_core::capture::Allocation;
use bediz_core::delivery::{CopyOrder, Delivery};
use bediz_core::file_name::Pattern;
use windows::Win32::Foundation::{HINSTANCE, HWND, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, HWND_MESSAGE, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_QUIT, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_TIMER, WaitMessage,
};
use windows::core::{Result, w};

use bediz_core::geometry::Rect;

use super::clock::Clock;
use super::contrast::{Colours, contrast_colours};
use super::desktop_switch::{self, DesktopSwitchHook};
use super::hotkey::{self, Change, Registration};
use super::label_window::LabelWindow;
use super::save_dialog::Answers;
use super::scene::Scene;
use super::session::Session;
use super::virtual_desktop::VirtualDesktops;
use super::window_class::{self, MESSAGE_CLASS};
use super::window_list::WindowList;
use super::worker::Worker;
use crate::event::{Command, Event, Failure, HotkeyState, MenuTexts};
use delivery::{PendingCopy, Tracked};
use save_as::{PendingSaveAs, Retry};

pub const WM_COMMANDS: u32 = WM_APP + 1;
pub const WM_STOP: u32 = WM_APP + 2;
pub const WM_OUTCOMES: u32 = WM_APP + 3;
pub const WM_SAVE_AS: u32 = WM_APP + 4;
pub const WM_SECOND_START: u32 = WM_APP + 5;
pub const WM_FINISH_SAVES: u32 = WM_APP + 6;

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
    hotkey: Registration,
    recording_hotkey: bool,
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
    saves_finished: Sender<()>,
    clock: Clock,
    allocation: Allocation,
    windows: WindowList,
    detect_windows: bool,
    enter: Delivery,
    save_folder: Option<PathBuf>,
    file_name: Pattern,
    keep_out_of_history: bool,
    colours: Option<Colours>,
    last_region: Option<Rect>,
}

pub fn run(
    commands: Commands,
    sink: Sink,
    ready: Sender<std::result::Result<isize, String>>,
    saves_finished: Sender<()>,
) {
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        let _ = ready.send(Err(error.to_string()));
        return;
    }
    match Runtime::create(commands, sink, saves_finished) {
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
    fn create(commands: Commands, sink: Sink, saves_finished: Sender<()>) -> Result<Self> {
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
            hotkey: Registration::new(window),
            recording_hotkey: false,
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
            saves_finished,
            clock: Clock::new(),
            allocation: Allocation::default(),
            windows: WindowList::new(),
            detect_windows: false,
            enter: Delivery::COPY_AND_SAVE,
            save_folder: None,
            file_name: Pattern::default(),
            keep_out_of_history: false,
            colours: contrast_colours(),
            last_region: None,
        })
    }

    fn start(&mut self) {
        let mut notes = Vec::new();
        self.hotkey.start_watching(&mut notes);
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
        self.hotkey.unregister();
        self.finish_saves();
        self.scene = None;
    }

    fn pump(&mut self) {
        loop {
            let event = self.hotkey.wake_event();
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
                    let change = self.hotkey.setting_changed(&mut notes);
                    self.emit_notes(notes);
                    self.report_hotkey(change);
                }
                WAIT_FAILED if event.is_some() => {
                    self.hotkey.stop_watching();
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
            match std::panic::catch_unwind(AssertUnwindSafe(|| self.drain())) {
                Ok(true) => {}
                Ok(false) => break,
                Err(_) => self.recover(),
            }
        }
    }

    // A panic must not end this thread: the hotkey and every overlay live on it.
    fn recover(&mut self) {
        (self.sink)(Event::Failed(Failure::Overlay(
            "an internal error was caught; the overlay was closed".to_owned(),
        )));
        window_class::guarded((), || self.hide());
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
            WM_HOTKEY if own && message.wParam.0 == hotkey::HOTKEY as usize => {
                self.on_hotkey(message);
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
            WM_SECOND_START if own => (self.sink)(Event::SecondStart),
            WM_FINISH_SAVES if own => {
                self.finish_saves();
                let _ = self.saves_finished.send(());
            }
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

    fn after_message(&mut self) -> bool {
        if window_class::take_close_requested() {
            self.hide();
        }
        if window_class::take_display_changed() {
            self.on_display_change();
        }
        if window_class::take_colours_changed() {
            self.on_colours_change();
        }
        if desktop_switch::take_switched() {
            self.hide();
            if let Some(scene) = self.scene.as_mut() {
                scene.reopen_capture();
            }
            let change = self.hotkey.refresh();
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
            if change.state != HotkeyState::Registered {
                (self.sink)(Event::Note(change.note));
            }
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
