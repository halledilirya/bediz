//! The thread that owns the overlay, pins, hotkeys, recordings, copies and saves: it handles their
//! messages, acts on the app's commands and reports what happens back to the app.

mod announce;
mod commands;
mod copies;
mod delay;
mod delivery;
mod discard;
mod displays;
mod hold;
mod markup_bar;
mod menu;
mod messages;
mod picture;
mod pins;
mod quit;
mod reading;
mod ready;
mod recording;
mod save_as;
mod screenshot;
mod typing;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use bediz_core::capture::Allocation;
use bediz_core::delivery::{CopyOrder, Delivery};
use bediz_core::file_name::Pattern;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
};
use windows::core::{Result, w};

use bediz_core::geometry::Rect;
use bediz_core::ocr::Layout;
use bediz_core::recording::RecordingOptions;

use super::clock::Clock;
use super::contrast::{Colours, contrast_colours};
use super::desktop_switch::DesktopSwitchHook;
use super::hotkey::{self, Change, Registration};
use super::label_window::LabelWindow;
use super::ocr::Readings;
use super::picture::Pictures;
use super::recording::{Recording, Timer};
use super::save_dialog::Answers;
use super::scene::Scene;
use super::session::Session;
use super::virtual_desktop::VirtualDesktops;
use super::window_class::{self, MESSAGE_CLASS, OwnedWindow};
use super::window_list::WindowList;
use super::worker::Worker;
use crate::event::{Command, Event, HotkeyState, Label, MenuTexts};
use announce::Heard;
use copies::PendingCopy;
use delay::Delay;
use delivery::Tracked;
use discard::Deletions;
use markup_bar::MarkupBar;
use pins::Pins;
use quit::Quitting;
use reading::{Asked, Turns};
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
pub const WM_EFFECT_MADE: u32 = WM_APP + 9;
pub const WM_PICTURE: u32 = WM_APP + 10;
pub const WM_TEXT: u32 = WM_APP + 11;
pub const WM_PIN_KEPT: u32 = WM_APP + 12;

const MENU_FADE_TIMER: usize = 1;
const DISPLAY_TIMER: usize = 2;
const CLIPBOARD_TIMER: usize = 3;
const LABEL_TIMER: usize = 4;
const DELAY_TIMER: usize = 5;
const DISPLAY_SETTLE_MILLISECONDS: u32 = 500;
const LABEL_MILLISECONDS: u32 = 1500;

pub type Commands = Arc<Mutex<Vec<Command>>>;
pub type Sink = Box<dyn Fn(Event) + Send>;

struct Runtime {
    session: Option<Session>,
    scene: Option<Scene>,
    displays_changed: bool,
    menu_capture: bool,
    delay: Option<Delay>,
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
    text_layout: Layout,
    text_language: Option<String>,
    colours: Option<Colours>,
    last_region: Option<Rect>,
    recording: Option<Recording>,
    recording_options: RecordingOptions,
    finishing: Vec<Finishing>,
    gifs: u64,
    gif_copy: Option<(u64, u64)>,
    quit: Option<Quitting>,
    deleting: Deletions,
    quitting: bool,
    live: Option<Timer>,
    ready: Option<ReadyBar>,
    markup_bar: Option<MarkupBar>,
    heard: Heard,
    pictures: Pictures,
    pictures_asked: u64,
    opening_picture: Option<u64>,
    readings: Readings,
    turns: Turns<Asked>,
    pins: Pins,
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
            delay: None,
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
            text_layout: Layout::default(),
            text_language: None,
            colours: contrast_colours(),
            last_region: None,
            recording: None,
            recording_options: RecordingOptions::default(),
            finishing: Vec::new(),
            gifs: 0,
            gif_copy: None,
            quit: None,
            deleting: Deletions::default(),
            quitting: false,
            live: None,
            ready: None,
            markup_bar: None,
            heard: Heard::default(),
            pictures: Pictures::default(),
            pictures_asked: 0,
            opening_picture: None,
            readings: Readings::default(),
            turns: Turns::default(),
            pins: Pins::default(),
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
        self.pins.close_every_window();
        self.hide();
        self.hotkey.unregister();
        self.gif_hotkey.unregister();
        self.finish_saves();
        self.scene = None;
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
