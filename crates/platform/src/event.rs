use bediz_core::capture::Allocation;
use bediz_core::delivery::{CopyError, SaveError};
use bediz_core::measurement::{Measurement, Memory};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Capture,
    CaptureAfterMenu,
    RetryHotkey,
    TakePrintScreen,
    SetAllocation(Allocation),
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
    #[error("capturing failed: {0}")]
    Capture(String),
    #[error("showing the overlay failed: {0}")]
    Overlay(String),
    #[error("encoding the image failed: {0}")]
    Encode(String),
    #[error("earlier screenshots are still being encoded")]
    Busy,
    #[error("copying failed: {0}")]
    Copy(CopyError),
    #[error("saving failed: {0}")]
    Save(SaveError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Hotkey(HotkeyState),
    Idle(Memory),
    Measured(Measurement),
    Copied { width: u32, height: u32 },
    Saved { width: u32, height: u32 },
    CopiedColour,
    Note(String),
    Failed(Failure),
}
