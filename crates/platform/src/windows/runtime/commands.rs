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
                Command::ShowLabel(label) => self.show_label(label),
                Command::SetMenuTexts(texts) => self.menu_texts = Some(texts),
                Command::Resume => self.resume(),
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

    // A label from a save that ended in the background must not cover the screen being selected.
    pub(super) fn show_label(&mut self, label: Label) {
        if self.session.is_some() {
            let waiting = self.label_after_overlay.take();
            self.label_after_overlay = Some(deferred(waiting, label));
            return;
        }
        let Some(window) = &self.label else {
            return;
        };
        if let Err(error) = window.show(&label, self.colours.as_ref()) {
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

// Errors not shown yet are joined, never dropped; each comes from a job asked for before the selection opened.
fn deferred(waiting: Option<Label>, label: Label) -> Label {
    match waiting {
        Some(waiting) if waiting.warning && !label.warning => waiting,
        Some(waiting) if waiting.warning => {
            let mut lines = waiting.lines;
            for line in &label.lines {
                if !lines.contains(line) {
                    lines.push(line.clone());
                }
            }
            Label { lines, ..label }
        }
        _ => label,
    }
}

#[cfg(test)]
mod tests {
    use bediz_core::geometry::Rect;

    use super::deferred;
    use crate::event::Label;

    fn label(lines: &[&str], warning: bool, left: i32) -> Label {
        Label {
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            near: Rect {
                left,
                top: 0,
                right: left + 10,
                bottom: 10,
            },
            warning,
            language: "en".to_owned(),
            right_to_left: false,
        }
    }

    #[test]
    fn a_success_never_takes_the_place_of_an_error_not_shown_yet() {
        let error = label(&["Not saved: the disk is full"], true, 0);
        let success = label(&["Copied"], false, 50);
        assert_eq!(deferred(Some(error.clone()), success), error);
    }

    #[test]
    fn an_error_takes_the_place_of_a_success_not_shown_yet() {
        let success = label(&["Copied"], false, 0);
        let error = label(&["Not saved: the disk is full"], true, 50);
        assert_eq!(deferred(Some(success.clone()), error.clone()), error);
        assert_eq!(deferred(None, success.clone()), success);
        let later = label(&["Saved"], false, 50);
        assert_eq!(deferred(Some(success), later.clone()), later);
    }

    #[test]
    fn errors_not_shown_yet_are_joined_once_each_near_the_latest() {
        let first = label(&["Copied", "Not saved: the disk is full"], true, 0);
        let second = label(&["Not copied: something unexpected happened"], true, 50);
        let again = label(&["Copied", "Not saved: the disk is full"], true, 90);
        let joined = deferred(Some(deferred(Some(first), second)), again);
        assert_eq!(
            joined.lines,
            [
                "Copied",
                "Not saved: the disk is full",
                "Not copied: something unexpected happened"
            ]
        );
        assert_eq!(joined.near.left, 90);
        assert!(joined.warning);
    }

    #[test]
    fn every_error_not_shown_yet_is_kept_in_order() {
        let mut waiting = None;
        for number in 0..10 {
            let line = format!("Not saved: error {number}");
            waiting = Some(deferred(waiting, label(&[&line], true, 0)));
            let success = label(&["Copied"], false, 0);
            waiting = Some(deferred(waiting, success));
        }
        let lines = waiting.map(|label| label.lines).unwrap_or_default();
        let expected: Vec<String> = (0..10)
            .map(|number| format!("Not saved: error {number}"))
            .collect();
        assert_eq!(lines, expected);
    }
}
