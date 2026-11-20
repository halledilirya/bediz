//! Commands the app sends to the platform thread and the events it sends back, with their payloads.

use std::path::PathBuf;
use std::time::Duration;

use bediz_core::capture::Allocation;
use bediz_core::delivery::{CopyError, Delivery, SaveError};
use bediz_core::file_name::Pattern;
use bediz_core::geometry::Rect;
use bediz_core::hotkey::Hotkey;
use bediz_core::markup::{Swatch, Tool};
use bediz_core::measurement::{Measurement, Memory};
use bediz_core::notice::Notice;
use bediz_core::ocr::Layout;
use bediz_core::recording::RecordingOptions;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Capture,
    CaptureAfterMenu,
    CaptureIn(Duration),
    CancelDelay,
    MarkUp(ImageSource),
    RecordGif,
    RetryHotkey,
    TakePrintScreen,
    SetHotkey {
        hotkey: Hotkey,
        yields: bool,
    },
    SetGifHotkey {
        hotkey: Option<Hotkey>,
        yields: bool,
    },
    RecordHotkey(bool),
    SetAllocation(Allocation),
    SetWindowDetection(bool),
    SetEnter(Delivery),
    SetSaving {
        folder: Option<PathBuf>,
        file_name: Pattern,
    },
    KeepOutOfHistory(bool),
    SetTextLayout(Layout),
    SetRecordingOptions(RecordingOptions),
    ReserveGifCopy {
        id: u64,
    },
    CopyGif {
        id: u64,
        path: PathBuf,
    },
    SetLastRegion(Option<Rect>),
    SetStartWithWindows {
        on: bool,
        request: u64,
    },
    ShowLabel(Label),
    SetMenuTexts(Box<MenuTexts>),
    Resume,
    Recording(RecordingAction),
    Quit(QuitStep),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSource {
    Clipboard,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitStep {
    Ask,
    WhenSaved,
    Discard,
    Stay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingAction {
    Pause,
    Resume,
    Finish,
    Restart,
    Discard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingState {
    Off,
    Going,
    Paused,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedGif {
    pub path: PathBuf,
    pub bytes: u64,
    pub length: Duration,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GifEvent {
    Saving {
        id: u64,
        near: Rect,
    },
    Written {
        id: u64,
        percent: u8,
    },
    Saved {
        id: u64,
        gif: SavedGif,
    },
    NotSaved {
        id: u64,
        error: SaveError,
    },
    Copied {
        id: u64,
        result: Result<(), CopyError>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuTexts {
    pub copy_and_save: String,
    pub copy: String,
    pub save: String,
    pub save_as: String,
    pub copy_colour: String,
    pub read_text: String,
    pub text_copied: String,
    pub join_lines: String,
    pub keep_indentation: String,
    pub whole_monitor: String,
    pub last_region: String,
    pub cancel: String,
    pub record: String,
    pub recording: String,
    pub start_recording: String,
    pub pause_recording: String,
    pub resume_recording: String,
    pub finish_recording: String,
    pub restart_recording: String,
    pub cancel_recording: String,
    pub discard_recording: String,
    pub lock_area: String,
    pub unlock_area: String,
    pub selection_size: String,
    pub images: String,
    pub markup: MarkupTexts,
    pub right_to_left: bool,
}

impl MenuTexts {
    pub const WIDTH: &str = "{width}";
    pub const HEIGHT: &str = "{height}";

    pub fn selection_size_of(&self, width: u32, height: u32) -> String {
        self.selection_size
            .replace(Self::WIDTH, &width.to_string())
            .replace(Self::HEIGHT, &height.to_string())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkupTexts {
    pub title: String,
    pub pen: String,
    pub highlighter: String,
    pub line: String,
    pub arrow: String,
    pub rectangle: String,
    pub ellipse: String,
    pub text: String,
    pub step: String,
    pub redact: String,
    pub blur: String,
    pub pixelate: String,
    pub no_tool: String,
    pub colour: String,
    pub undo: String,
    pub redo: String,
    pub red: String,
    pub orange: String,
    pub yellow: String,
    pub green: String,
    pub blue: String,
    pub purple: String,
    pub black: String,
    pub white: String,
}

impl MarkupTexts {
    pub fn tool(&self, tool: Option<Tool>) -> &str {
        match tool {
            None => &self.no_tool,
            Some(Tool::Pen) => &self.pen,
            Some(Tool::Highlighter) => &self.highlighter,
            Some(Tool::Line) => &self.line,
            Some(Tool::Arrow) => &self.arrow,
            Some(Tool::Rectangle) => &self.rectangle,
            Some(Tool::Ellipse) => &self.ellipse,
            Some(Tool::Text) => &self.text,
            Some(Tool::Step) => &self.step,
            Some(Tool::Redact) => &self.redact,
            Some(Tool::Blur) => &self.blur,
            Some(Tool::Pixelate) => &self.pixelate,
        }
    }

    pub fn swatch(&self, swatch: Swatch) -> &str {
        match swatch {
            Swatch::Red => &self.red,
            Swatch::Orange => &self.orange,
            Swatch::Yellow => &self.yellow,
            Swatch::Green => &self.green,
            Swatch::Blue => &self.blue,
            Swatch::Purple => &self.purple,
            Swatch::Black => &self.black,
            Swatch::White => &self.white,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub lines: Vec<String>,
    pub near: Rect,
    pub warning: bool,
    pub at_once: bool,
    pub language: String,
    pub right_to_left: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyState {
    Registered,
    SnippingTool,
    Taken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Region,
    Gif,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum Failure {
    #[error("starting the displays failed: {0}")]
    Displays(String),
    #[error("showing the overlay failed: {0}")]
    Overlay(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Hotkey(HotkeyState),
    GifHotkey(HotkeyState),
    PrintScreenYields,
    HotkeyWhileRecording(Shortcut),
    LastRegion(Rect),
    TextLayout(Layout),
    StartWithWindows {
        on: bool,
        request: u64,
        failure: Option<String>,
    },
    Idle(Memory),
    Measured(Measurement),
    Notice {
        notices: Vec<Notice>,
        near: Rect,
        at_once: bool,
    },
    Note(String),
    Failed(Failure),
    SecondStart,
    Recording(RecordingState),
    Delaying(bool),
    Gif(GifEvent),
    Unsaved(Option<u8>),
    ReadyToQuit,
    QuitCancelled,
}
