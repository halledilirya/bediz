use std::sync::PoisonError;
use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{LABEL_MILLISECONDS, LABEL_TIMER, Runtime, WARNING_MILLISECONDS};
use crate::event::{Command, Event, Label};
use crate::windows::startup;

impl Runtime {
    pub(super) fn on_commands(&mut self) {
        let commands =
            std::mem::take(&mut *self.commands.lock().unwrap_or_else(PoisonError::into_inner));
        for command in commands {
            match command {
                Command::Capture => self.open(Duration::ZERO),
                Command::CaptureAfterMenu => self.open_after_menu(),
                Command::RetryHotkey => {
                    let change = self.hotkey.refresh();
                    self.report_hotkey(change);
                }
                Command::TakePrintScreen => {
                    let mut notes = Vec::new();
                    let change = self.hotkey.take_from_snipping_tool(&mut notes);
                    self.emit_notes(notes);
                    self.report_hotkey(change);
                }
                Command::SetHotkey { hotkey, yields } => {
                    let change = self.hotkey.set(hotkey, yields);
                    self.report_hotkey(change);
                }
                Command::RecordHotkey(recording) => self.recording_hotkey = recording,
                Command::SetAllocation(allocation) => self.set_allocation(allocation),
                Command::SetWindowDetection(detect) => self.detect_windows = detect,
                Command::SetEnter(delivery) => self.enter = delivery,
                Command::SetSaving { folder, file_name } => {
                    self.save_folder = folder;
                    self.file_name = file_name;
                }
                Command::KeepOutOfHistory(keep_out) => self.keep_out_of_history = keep_out,
                Command::SetLastRegion(region) => self.last_region = region,
                Command::SetStartWithWindows { on, request } => {
                    self.start_with_windows(on, request);
                }
                Command::ShowLabel(label) => self.show_label(&label),
                Command::SetMenuTexts(texts) => self.menu_texts = Some(texts),
            }
        }
    }

    fn start_with_windows(&self, on: bool, request: u64) {
        let failure = startup::start_with_windows(on)
            .err()
            .map(|error| error.to_string());
        (self.sink)(Event::StartWithWindows {
            on,
            request,
            failure,
        });
    }

    fn show_label(&mut self, label: &Label) {
        let Some(window) = &self.label else {
            return;
        };
        if let Err(error) = window.show(label, self.colours.as_ref()) {
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

    pub(super) fn hide_label(&mut self) {
        let _ = unsafe { KillTimer(Some(self.window.0), LABEL_TIMER) };
        if let Some(window) = &self.label {
            window.hide();
        }
    }
}
