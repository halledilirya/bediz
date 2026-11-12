mod commands;
mod delivery;
mod displays;
mod hold;
mod ready;
mod recording;
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
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, HWND_MESSAGE, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_QUIT, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_TIMER, WaitMessage,
};
use windows::core::{Result, w};

use bediz_core::geometry::Rect;
use bediz_core::recording::{RecordingOptions, RecordingStop};

use super::clock::{self, Clock};
use super::contrast::{Colours, contrast_colours};
use super::desktop_switch::{self, DesktopSwitchHook};
use super::hotkey::{self, Change, Registration};
use super::label_window::LabelWindow;
use super::recording::{Recording, Timer};
use super::save_dialog::Answers;
use super::scene::Scene;
use super::session::Purpose;
use super::session::Session;
use super::virtual_desktop::VirtualDesktops;
use super::window_class::{self, MESSAGE_CLASS, OwnedWindow};
use super::window_list::WindowList;
use super::worker::Worker;
use crate::event::{Command, Event, Failure, HotkeyState, Label, MenuTexts};
use delivery::{PendingCopy, Tracked};
use ready::ReadyBar;
use recording::Finishing;
use save_as::{PendingSaveAs, Waiting};

pub const WM_COMMANDS: u32 = WM_APP + 1;
pub const WM_STOP: u32 = WM_APP + 2;
pub const WM_OUTCOMES: u32 = WM_APP + 3;
pub const WM_SAVE_AS: u32 = WM_APP + 4;
pub const WM_SECOND_START: u32 = WM_APP + 5;
pub const WM_FINISH_SAVES: u32 = WM_APP + 6;
pub const WM_HOLD: u32 = WM_APP + 7;
pub const WM_RECORDING: u32 = WM_APP + 8;

const MENU_FADE_TIMER: usize = 1;
const DISPLAY_TIMER: usize = 2;
const CLIPBOARD_TIMER: usize = 3;
const LABEL_TIMER: usize = 4;
const DISPLAY_SETTLE_MILLISECONDS: u32 = 500;
const LABEL_MILLISECONDS: u32 = 1500;

pub type Commands = Arc<Mutex<Vec<Command>>>;
pub type Sink = Box<dyn Fn(Event) + Send>;

struct Runtime {
    session: Option<Session>,
    scene: Option<Scene>,
    displays_changed: bool,
    menu_capture: bool,
    held: bool,
    hotkey: Registration,
    gif_hotkey: Registration,
    recording_hotkey: bool,
    desktop_switch: Option<DesktopSwitchHook>,
    desktops: Option<VirtualDesktops>,
    worker: Option<Worker>,
    copies: CopyOrder,
    clipboard: Option<PendingCopy>,
    deliveries: u64,
    reports: Vec<Tracked>,
    label: Option<LabelWindow>,
    label_after_overlay: Option<Label>,
    menu_texts: Option<MenuTexts>,
    save_as: Option<PendingSaveAs>,
    save_as_answers: Answers,
    waiting: VecDeque<Waiting>,
    window: OwnedWindow,
    instance: HINSTANCE,
    commands: Commands,
    sink: Sink,
    saves_finished: Sender<bool>,
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
    recording: Option<Recording>,
    recording_options: RecordingOptions,
    finishing: Vec<Finishing>,
    gifs: u64,
    gif_copy: Option<(u64, u64)>,
    quitting: bool,
    live: Option<Timer>,
    ready: Option<ReadyBar>,
}

fn handles(event: Option<HANDLE>, timer: Option<HANDLE>) -> ([HANDLE; 2], usize) {
    match (event, timer) {
        (Some(event), Some(timer)) => ([event, timer], 2),
        (Some(only), None) | (None, Some(only)) => ([only, HANDLE::default()], 1),
        (None, None) => ([HANDLE::default(); 2], 0),
    }
}

pub fn run(
    commands: Commands,
    sink: Sink,
    ready: Sender<std::result::Result<isize, String>>,
    saves_finished: Sender<bool>,
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
    fn create(commands: Commands, sink: Sink, saves_finished: Sender<bool>) -> Result<Self> {
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
            held: false,
            hotkey: Registration::new(window, hotkey::HOTKEY),
            gif_hotkey: Registration::new(window, hotkey::GIF_HOTKEY),
            recording_hotkey: false,
            desktop_switch: DesktopSwitchHook::install(),
            desktops: None,
            worker: None,
            copies: CopyOrder::default(),
            clipboard: None,
            deliveries: 0,
            reports: Vec::new(),
            label: None,
            label_after_overlay: None,
            menu_texts: None,
            save_as: None,
            save_as_answers: Answers::default(),
            waiting: VecDeque::new(),
            window: OwnedWindow(window),
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
            recording: None,
            recording_options: RecordingOptions::default(),
            finishing: Vec::new(),
            gifs: 0,
            gif_copy: None,
            quitting: false,
            live: None,
            ready: None,
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
        self.start_worker(&mut notes);
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
        self.gif_hotkey.unregister();
        self.finish_saves();
        self.scene = None;
    }

    fn pump(&mut self) {
        loop {
            let event = self.hotkey.wake_event();
            let timer = self
                .recording
                .as_ref()
                .and_then(Recording::timer)
                .or_else(|| self.live.as_ref().map(Timer::handle));
            let (all, count) = handles(event, timer);
            let handles = all.get(..count).unwrap_or_default();
            let waited = unsafe {
                MsgWaitForMultipleObjectsEx(
                    Some(handles),
                    INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                )
            };
            let signalled = usize::try_from(waited.0.wrapping_sub(WAIT_OBJECT_0.0))
                .ok()
                .and_then(|index| handles.get(index))
                .copied();
            if waited == WAIT_FAILED {
                self.wait_failed(event.is_some(), timer.is_some());
            } else if signalled.is_some() && signalled == event {
                let mut notes = Vec::new();
                let change = self.hotkey.setting_changed(&mut notes);
                let gif_change = self.gif_hotkey.setting_changed(&mut notes);
                self.emit_notes(notes);
                self.report_hotkey(change);
                self.report_gif_hotkey(gif_change);
            } else if signalled.is_some()
                && signalled == timer
                && std::panic::catch_unwind(AssertUnwindSafe(|| self.on_timer())).is_err()
            {
                self.recover();
            }
            match std::panic::catch_unwind(AssertUnwindSafe(|| self.drain())) {
                Ok(true) => {}
                Ok(false) => break,
                Err(_) => self.recover(),
            }
        }
    }

    fn on_timer(&mut self) {
        if self.recording.is_some() {
            self.on_recording_tick();
        } else {
            self.on_live_tick();
        }
    }

    fn wait_failed(&mut self, watching: bool, timing: bool) {
        if watching {
            self.hotkey.stop_watching();
            self.emit_notes(vec![
                "waiting on the Print Screen setting failed; it is no longer watched".to_owned(),
            ]);
        } else if timing {
            self.emit_notes(vec![
                "waiting on a recording's timer failed; it was stopped".to_owned(),
            ]);
            self.live = None;
            self.stop_recording(None);
        } else {
            let _ = unsafe { WaitMessage() };
        }
    }

    // A panic must not end this thread: the hotkey and every overlay live on it.
    fn recover(&mut self) {
        (self.sink)(Event::Failed(Failure::Overlay(
            "an internal error was caught; the overlay was closed".to_owned(),
        )));
        window_class::guarded((), || self.hide());
        window_class::guarded((), || self.stop_recording(None));
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
                self.on_hotkey(message.lParam, Some(clock::message_age(message.time)));
            }
            WM_HOTKEY if own && message.wParam.0 == hotkey::GIF_HOTKEY as usize => {
                self.on_gif_hotkey(clock::message_age(message.time));
            }
            WM_TIMER if own && message.wParam.0 == MENU_FADE_TIMER => {
                if self.menu_capture {
                    self.open(Duration::ZERO, Purpose::Screenshot);
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
            WM_FINISH_SAVES if own => self.on_finish_saves(),
            WM_HOLD if own => self.on_hold(),
            WM_RECORDING if own => self.on_recording_ended(),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_RBUTTONUP | WM_MOUSEMOVE
            | WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
                if self.is_overlay(message.hwnd) && !self.locked() =>
            {
                self.on_input(message);
            }
            // A click on a start button can move the keyboard focus there, away from the selection.
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
                if self
                    .ready
                    .as_ref()
                    .is_some_and(|bar| bar.owns(message.hwnd) && !bar.is_locked()) =>
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
        if window_class::take_label_close_requested() {
            self.hide_label();
        }
        if window_class::take_display_changed() {
            self.on_display_change();
        }
        if window_class::take_colours_changed() {
            self.on_colours_change();
        }
        if let Some(request) = window_class::take_controls_request() {
            self.on_controls(request);
        }
        if desktop_switch::take_switched() {
            self.stop_recording(Some(RecordingStop::Display));
            self.hide();
            if let Some(scene) = self.scene.as_mut() {
                scene.reopen_capture();
            }
            let change = self.hotkey.refresh();
            self.report_hotkey(change);
            let change = self.gif_hotkey.refresh();
            self.report_gif_hotkey(change);
        }
        // A press the overlay's menu took acts in the overlay still open, but never opens a new one.
        if let Some(keys) = window_class::take_missed_hotkey() {
            self.on_hotkey(keys, None);
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

    fn report_gif_hotkey(&self, change: Option<Change>) {
        if let Some(change) = change {
            if change.state != HotkeyState::Registered {
                (self.sink)(Event::Note(change.note));
            }
            (self.sink)(Event::GifHotkey(change.state));
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
