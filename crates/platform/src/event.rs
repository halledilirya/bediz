use bediz_core::capture::Allocation;
use bediz_core::geometry::Rect;
use bediz_core::measurement::{Measurement, Memory};
use bediz_core::notice::Notice;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Capture,
    CaptureAfterMenu,
    RetryHotkey,
    TakePrintScreen,
    SetAllocation(Allocation),
    ShowLabel(Label),
    SetMenuTexts(MenuTexts),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuTexts {
    pub copy_and_save: String,
    pub copy: String,
    pub save: String,
    pub save_as: String,
    pub copy_colour: String,
    pub whole_monitor: String,
    pub last_region: String,
    pub cancel: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub lines: Vec<String>,
    pub near: Rect,
    pub warning: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyState {
    Registered,
    SnippingTool,
    Taken,
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
    Idle(Memory),
    Measured(Measurement),
    Notice { notices: Vec<Notice>, near: Rect },
    Note(String),
    Failed(Failure),
}
