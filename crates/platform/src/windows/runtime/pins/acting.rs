//! What is asked of one pin: its mouse, keys and window events, a shortcut pressed on it, its menu,
//! and copying, saving or closing it.

use bediz_core::delivery::Delivery;
use bediz_core::geometry::Point;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, MSG};

use crate::event::PinAction;
use crate::windows::hotkey;
use crate::windows::menu_capture;
use crate::windows::pin::menu::{self, Choice};
use crate::windows::pin::{Asked, Pin, PinEvent};
use crate::windows::recording::Next;
use crate::windows::runtime::Runtime;
use crate::windows::window_class;

impl Runtime {
    pub(crate) fn on_pin_input(&mut self, message: &MSG) {
        let Some(id) = self.pins.id_of(message.hwnd) else {
            return;
        };
        match self.with_shown_pin(id, |pin, devices| pin.handle(devices, message)) {
            Ok(asked) => self.act_on_pin(id, asked),
            Err(error) => self.pin_trouble(&error),
        }
    }

    pub(crate) fn on_pin_events(&mut self) {
        for (window, event) in window_class::take_pin_events() {
            let Some(id) = self.pins.id_of(window) else {
                continue;
            };
            let done = match event {
                PinEvent::Close => {
                    self.close_pin(id);
                    Ok(())
                }
                PinEvent::Activated(focused) => {
                    let done =
                        self.with_shown_pin(id, |pin, devices| pin.activated(devices, focused));
                    if focused && self.pins.shelf.is_shown(id) {
                        self.keep_frames_on_top();
                    }
                    done
                }
                PinEvent::Rescaled => self.with_shown_pin(id, |pin, devices| pin.rescale(devices)),
                PinEvent::Painted => self.recover_pins().map(drop),
            };
            if let Err(error) = done {
                self.pin_trouble(&error);
            }
        }
    }

    // The shortcut is taken from every window, a pin too, so on the pin in front it acts as its key.
    pub(crate) fn press_in_pin(&mut self, keys: LPARAM) -> bool {
        let press = hotkey::as_key_press(keys);
        if !Pin::takes(press) {
            return false;
        }
        let front = unsafe { GetForegroundWindow() };
        let Some(id) = self.pins.id_of(front) else {
            return false;
        };
        if !self.pins.shelf.is_shown(id) {
            return false;
        }
        match self.with_shown_pin(id, |pin, devices| pin.press(devices, press)) {
            Ok(asked) => self.act_on_pin(id, asked),
            Err(error) => self.pin_trouble(&error),
        }
        true
    }

    fn act_on_pin(&mut self, id: u64, asked: Asked) {
        match asked {
            Asked::Nothing => {}
            Asked::Close => self.close_pin(id),
            Asked::Copy => self.deliver_pin(id, Delivery::COPY),
            Asked::Save => self.deliver_pin(id, Delivery::SAVE),
            Asked::SaveAs => {
                if let Some(pin) = self.pins.find(id) {
                    let (near, pixels, size, window) =
                        (pin.rect(), pin.pixels(), pin.size(), pin.window());
                    self.save_pin_as(near, pixels, size, window);
                }
            }
            Asked::Menu(at) => self.open_pin_menu(id, at),
        }
    }

    fn close_pin(&mut self, id: u64) {
        let changes = self.pins.shelf.close(id);
        self.apply_pins(changes);
    }

    fn deliver_pin(&mut self, id: u64, delivery: Delivery) {
        if let Some(pin) = self.pins.find(id) {
            let (near, pixels, size) = (pin.rect(), pin.pixels(), pin.size());
            self.deliver_pixels(near, pixels, size, delivery);
        }
    }

    fn open_pin_menu(&mut self, id: u64, at: Point) {
        let (Some(texts), Some(pin)) = (self.menu_texts.as_ref(), self.pins.find(id)) else {
            return;
        };
        let look = pin.look();
        let kept_out = !self.pins.in_captures;
        let choose = || menu_capture::kept_out_while(kept_out, || menu::choose(pin, at, texts));
        let mut notes = Vec::new();
        let held = match (self.recording.as_mut(), self.scene.as_mut()) {
            (Some(recording), Some(scene)) => recording.record_while(scene, &mut notes, choose),
            _ => Ok((choose(), Next::Going)),
        };
        let ((chosen, note), next) = match held {
            Ok(held) => held,
            Err(error) => {
                self.emit_notes(vec![format!(
                    "the pin's menu was not opened, as it would hold up the recording: {error}"
                )]);
                return;
            }
        };
        notes.extend(note);
        self.emit_notes(notes);
        if let Next::Stop(stop) = next {
            self.stop_recording(stop);
        }
        let chosen = match chosen {
            Ok(Some(chosen)) => chosen,
            Ok(None) => return,
            Err(error) => {
                self.pin_trouble(&error);
                return;
            }
        };
        let changed = match chosen {
            Choice::Copy => {
                self.act_on_pin(id, Asked::Copy);
                Ok(())
            }
            Choice::Save => {
                self.act_on_pin(id, Asked::Save);
                Ok(())
            }
            Choice::SaveAs => {
                self.act_on_pin(id, Asked::SaveAs);
                Ok(())
            }
            Choice::ZoomIn => {
                self.with_shown_pin(id, |pin, devices| pin.zoom(devices, 1, false, None))
            }
            Choice::ZoomOut => {
                self.with_shown_pin(id, |pin, devices| pin.zoom(devices, -1, false, None))
            }
            Choice::ActualSize => self.with_shown_pin(id, Pin::show_actual_size),
            Choice::Fill => self.with_shown_pin(id, Pin::fill),
            Choice::Opacity(percent) => self.with_shown_pin(id, |pin, devices| {
                pin.change_look(devices, look.with_opacity(percent), None)
            }),
            Choice::Close => {
                self.close_pin(id);
                Ok(())
            }
            Choice::HideAll => {
                let changes = self.pins.shelf.hide_all();
                self.apply_pins(changes);
                Ok(())
            }
            Choice::CloseAll => {
                self.on_pin_action(PinAction::CloseAll);
                Ok(())
            }
        };
        if let Err(error) = changed {
            self.pin_trouble(&error);
        }
    }
}
