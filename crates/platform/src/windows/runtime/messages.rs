//! The runtime's loop: waits on messages, the Print Screen setting, the timer of a recording or of
//! the live screen and the overlay's last frame, hands each message to what handles it, the
//! overlay's input or a pin's, and keeps a panic from ending the thread.

use std::panic::AssertUnwindSafe;
use std::time::Duration;

use bediz_core::recording::RecordingStop;
use windows::Win32::Foundation::{HANDLE, HWND, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_F10;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, TranslateMessage, WM_CHAR, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_QUIT, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WaitMessage,
};

use super::{
    CLIPBOARD_TIMER, DELAY_TIMER, DISPLAY_TIMER, LABEL_TIMER, MENU_FADE_TIMER, Runtime,
    WM_COMMANDS, WM_EFFECT_MADE, WM_FINISH_SAVES, WM_HOLD, WM_OUTCOMES, WM_PICTURE, WM_PIN_KEPT,
    WM_RECORDING, WM_SAVE_AS, WM_SECOND_START, WM_STOP, WM_TEXT,
};
use crate::event::{Event, Failure};
use crate::windows::clock;
use crate::windows::desktop_switch;
use crate::windows::hotkey;
use crate::windows::overlay::SHOWN_WITHIN_MS;
use crate::windows::recording::{Recording, Timer};
use crate::windows::session::{Purpose, Session};
use crate::windows::window_class;

fn handles(event: Option<HANDLE>, timer: Option<HANDLE>) -> ([HANDLE; 2], usize) {
    match (event, timer) {
        (Some(event), Some(timer)) => ([event, timer], 2),
        (Some(only), None) | (None, Some(only)) => ([only, HANDLE::default()], 1),
        (None, None) => ([HANDLE::default(); 2], 0),
    }
}

impl Runtime {
    pub(super) fn pump(&mut self) {
        loop {
            let event = self.hotkey.wake_event();
            let timer = self.timer();
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

    fn timer(&self) -> Option<HANDLE> {
        self.recording
            .as_ref()
            .and_then(Recording::timer)
            .or_else(|| self.live.as_ref().map(Timer::handle))
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
        loop {
            self.wait_for_frames();
            if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if message.message == WM_QUIT || !self.dispatch(&message) || !self.after_message() {
                return false;
            }
            // While the pointer moves over a redrawn overlay the queue never empties for the wait.
            if self
                .timer()
                .is_some_and(|timer| unsafe { WaitForSingleObject(timer, 0) } == WAIT_OBJECT_0)
            {
                self.on_timer();
            }
        }
        // Sent messages and WinEvents run inside PeekMessageW, which then returns no message.
        self.after_message()
    }

    // Input read once the last frame is on the screen is drawn a frame sooner.
    fn wait_for_frames(&self) {
        if let Some(scene) = &self.scene
            && !scene.wait_for_frames()
        {
            self.emit_notes(vec![format!(
                "a frame was not on the screen within {SHOWN_WITHIN_MS} ms; the overlay draws \
                 without waiting for frames until it opens again"
            )]);
        }
    }

    fn dispatch(&mut self, message: &MSG) -> bool {
        let own = message.hwnd == self.window.0;
        match message.message {
            WM_STOP if own => return false,
            WM_HOTKEY if own && message.wParam.0 == hotkey::HOTKEY as usize => {
                self.on_hotkey(message.lParam, Some(clock::message_age(message.time)));
            }
            WM_HOTKEY if own && message.wParam.0 == hotkey::GIF_HOTKEY as usize => {
                self.on_gif_hotkey(message.lParam, Some(clock::message_age(message.time)));
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
            WM_TIMER if own && message.wParam.0 == DELAY_TIMER => self.tick_delay(),
            WM_COMMANDS if own => self.on_commands(),
            WM_OUTCOMES if own => self.on_outcomes(),
            WM_SAVE_AS if own => self.on_save_as_answer(),
            WM_SECOND_START if own => (self.sink)(Event::SecondStart),
            WM_FINISH_SAVES if own => self.on_finish_saves(),
            WM_HOLD if own => self.on_hold(),
            WM_RECORDING if own => self.on_recording_ended(),
            WM_EFFECT_MADE if own => self.reselect(|_, _| {}),
            WM_PICTURE if own => self.on_picture(),
            WM_TEXT if own => self.on_text(),
            WM_PIN_KEPT if own => self.on_pin_kept(),
            // Alt+F4 and the other system keys still reach the window, which closes a pin softly.
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_MOUSEMOVE
            | WM_MOUSEWHEEL | WM_KEYDOWN | WM_SYSKEYDOWN
                if self.pins.owns(message.hwnd)
                    && (message.message != WM_SYSKEYDOWN
                        || message.wParam.0 == usize::from(VK_F10.0)) =>
            {
                self.on_pin_input(message);
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP
            | WM_MOUSEMOVE | WM_MOUSEWHEEL | WM_CHAR | WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN
            | WM_SYSKEYUP
                if self.takes_input(message) =>
            {
                // Only a translated key brings its character and gives an input method its keys.
                if message.message == WM_KEYDOWN
                    && self.session.as_ref().is_some_and(Session::is_typing)
                {
                    let _ = unsafe { TranslateMessage(message) };
                }
                self.on_input(message);
            }
            _ => unsafe {
                let _ = TranslateMessage(message);
                DispatchMessageW(message);
            },
        }
        true
    }

    pub(super) fn is_overlay(&self, window: HWND) -> bool {
        self.scene.as_ref().is_some_and(|scene| scene.owns(window))
    }

    fn takes_input(&self, message: &MSG) -> bool {
        let window = message.hwnd;
        let keys = matches!(
            message.message,
            WM_CHAR | WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
        );
        // A click on a bar's button moves the keyboard focus there, away from the selection.
        let on_bar = self.ready.as_ref().is_some_and(|bar| bar.owns(window))
            || self.markup_bar.as_ref().is_some_and(|bar| bar.owns(window));
        !self.locked() && (self.is_overlay(window) || (keys && on_bar))
    }

    fn after_message(&mut self) -> bool {
        if window_class::take_close_requested() {
            self.hide();
        }
        if let Some(window) = window_class::take_label_close_requested() {
            self.close_label(window);
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
        if let Some(window) = window_class::take_mouse_left() {
            let left = MSG {
                hwnd: window,
                message: WM_MOUSELEAVE,
                ..Default::default()
            };
            if self.takes_input(&left) {
                self.on_input(&left);
            }
        }
        self.on_pin_events();
        self.follow_input_method();
        if desktop_switch::take_switched() {
            self.stop_recording(Some(RecordingStop::Display));
            self.hide();
            self.cancel_delay();
            if let Some(scene) = self.scene.as_mut() {
                scene.reopen_capture();
            }
            let change = self.hotkey.refresh();
            self.report_hotkey(change);
            let change = self.gif_hotkey.refresh();
            self.report_gif_hotkey(change);
        }
        // A press a menu took acts on a recording, overlay or pin still open, but never opens one.
        if let Some(keys) = window_class::take_missed_hotkey() {
            self.on_hotkey(keys, None);
        }
        if let Some(keys) = window_class::take_missed_gif_hotkey() {
            self.on_gif_hotkey(keys, None);
        }
        self.tell_quit();
        let own = self.window.0;
        window_class::take_missed().into_iter().all(|message| {
            self.dispatch(&MSG {
                hwnd: own,
                message,
                ..Default::default()
            })
        })
    }
}
