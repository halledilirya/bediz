//! A pin's mouse and keys: dragging it by hand, zooming and fading it with the wheel, and the keys
//! it takes once clicked, a shortcut pressed on it included.

use bediz_core::geometry::Point;
use bediz_core::pin::Wheel;
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_LBUTTON, MK_SHIFT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_0, VK_ADD, VK_APPS, VK_C, VK_CONTROL,
    VK_DOWN, VK_ESCAPE, VK_F10, VK_LEFT, VK_MENU, VK_NUMPAD0, VK_OEM_MINUS, VK_OEM_PLUS, VK_RIGHT,
    VK_S, VK_SHIFT, VK_SUBTRACT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MSG, SM_CXDRAG, SM_CYDRAG, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowPos, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_RBUTTONUP, WM_SYSKEYDOWN,
};
use windows::core::Result;

use super::{Asked, Devices, Pin};
use crate::windows::session::{KeyPress, WAS_DOWN};

const LONG_NUDGE: i32 = 10;

pub(super) struct Press {
    from: Point,
    at: Point,
    dragged: bool,
}

impl Pin {
    pub fn handle(&mut self, devices: &Devices, message: &MSG) -> Result<Asked> {
        let point = Point::new(message.pt.x, message.pt.y);
        match message.message {
            WM_LBUTTONDOWN => {
                unsafe { SetCapture(self.window.0) };
                self.press = Some(Press {
                    from: point,
                    at: self.at,
                    dragged: false,
                });
            }
            // A release lost with the capture, to a locked screen say, was no click.
            WM_MOUSEMOVE if message.wParam.0 & MK_LBUTTON.0 as usize == 0 => self.release(false),
            WM_MOUSEMOVE => self.drag(point)?,
            WM_LBUTTONUP => self.release(true),
            WM_MOUSEWHEEL => {
                let delta = i32::from((message.wParam.0 >> 16) as u16 as i16);
                let keys = message.wParam.0;
                if keys & MK_CONTROL.0 as usize != 0 {
                    let steps = self.fade_wheel.turn(delta);
                    self.change_look(devices, self.look.faded(steps), None)?;
                } else if self
                    .detent
                    .holds(message.time, delta > 0, self.fills_monitor())
                {
                    self.zoom_wheel = Wheel::default();
                } else {
                    let steps = self.zoom_wheel.turn(delta);
                    let fine = keys & MK_SHIFT.0 as usize != 0;
                    self.zoom(devices, steps, fine, Some(point))?;
                }
            }
            WM_MBUTTONUP => self.change_look(devices, self.made, Some(point))?,
            WM_RBUTTONUP => return Ok(Asked::Menu(point)),
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let press = KeyPress {
                    key: VIRTUAL_KEY(message.wParam.0 as u16),
                    alt: is_down(VK_MENU),
                    control: is_down(VK_CONTROL),
                    shift: is_down(VK_SHIFT),
                    repeat: message.lParam.0 & WAS_DOWN != 0,
                };
                return self.press(devices, press);
            }
            _ => {}
        }
        Ok(Asked::Nothing)
    }

    pub fn takes(press: KeyPress) -> bool {
        command(press).is_some()
    }

    pub fn press(&mut self, devices: &Devices, press: KeyPress) -> Result<Asked> {
        let Some(command) = command(press) else {
            return Ok(Asked::Nothing);
        };
        match command {
            Command::Close => return Ok(Asked::Close),
            Command::Copy => return Ok(Asked::Copy),
            Command::Save => return Ok(Asked::Save),
            Command::SaveAs => return Ok(Asked::SaveAs),
            Command::Menu => return Ok(Asked::Menu(self.middle())),
            Command::Nudge(dx, dy) => self.nudge(dx, dy)?,
            Command::Zoom(steps) => self.zoom(devices, steps, false, None)?,
            Command::Fade(steps) => self.change_look(devices, self.look.faded(steps), None)?,
            Command::ActualSize => self.show_actual_size(devices)?,
        }
        Ok(Asked::Nothing)
    }

    fn nudge(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.at = Point::new(self.at.x.saturating_add(dx), self.at.y.saturating_add(dy));
        self.home = self.at;
        self.move_window()
    }

    // The window is moved by hand: the system's move loop would stop the runtime's own pump.
    fn drag(&mut self, point: Point) -> Result<()> {
        let Some(press) = &mut self.press else {
            return Ok(());
        };
        let (dx, dy) = (point.x - press.from.x, point.y - press.from.y);
        if !press.dragged {
            let (reach_x, reach_y) =
                unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
            if dx.abs() < reach_x && dy.abs() < reach_y {
                return Ok(());
            }
            press.dragged = true;
        }
        self.at = Point::new(press.at.x.saturating_add(dx), press.at.y.saturating_add(dy));
        self.move_window()
    }

    fn move_window(&self) -> Result<()> {
        let rect = self.rect();
        unsafe {
            SetWindowPos(
                self.window.0,
                None,
                rect.left,
                rect.top,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE,
            )
        }
    }

    pub(super) fn release(&mut self, clicked: bool) {
        let Some(press) = self.press.take() else {
            return;
        };
        let _ = unsafe { ReleaseCapture() };
        if press.dragged {
            self.home = self.at;
        } else if clicked {
            self.focus();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Close,
    Copy,
    Save,
    SaveAs,
    Menu,
    Nudge(i32, i32),
    Zoom(i32),
    Fade(i32),
    ActualSize,
}

// Each key matches its modifiers exactly, so a shortcut such as Ctrl+Shift+C stays a shortcut.
fn command(press: KeyPress) -> Option<Command> {
    let KeyPress {
        key,
        alt,
        control,
        shift,
        repeat,
    } = press;
    let bare = !alt && !control;
    let once = |command| (!repeat).then_some(command);
    let step = if shift { LONG_NUDGE } else { 1 };
    match key {
        VK_ESCAPE if bare && !shift => once(Command::Close),
        VK_C if control && !alt && !shift => once(Command::Copy),
        VK_S if control && !alt && shift => once(Command::SaveAs),
        VK_S if control && !alt => once(Command::Save),
        VK_APPS if bare && !shift => once(Command::Menu),
        VK_F10 if bare && shift => once(Command::Menu),
        VK_LEFT if bare => Some(Command::Nudge(-step, 0)),
        VK_RIGHT if bare => Some(Command::Nudge(step, 0)),
        VK_UP if bare => Some(Command::Nudge(0, -step)),
        VK_DOWN if bare => Some(Command::Nudge(0, step)),
        // A plus sign takes Shift on some layouts, so Shift is left out of these.
        VK_OEM_PLUS | VK_ADD if control && !alt => Some(Command::Fade(1)),
        VK_OEM_MINUS | VK_SUBTRACT if control && !alt => Some(Command::Fade(-1)),
        VK_OEM_PLUS | VK_ADD if bare => Some(Command::Zoom(1)),
        VK_OEM_MINUS | VK_SUBTRACT if bare => Some(Command::Zoom(-1)),
        VK_0 | VK_NUMPAD0 if control && !alt && !shift => Some(Command::ActualSize),
        _ => None,
    }
}

fn is_down(key: VIRTUAL_KEY) -> bool {
    let state = unsafe { GetKeyState(i32::from(key.0)) };
    state < 0
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_SNAPSHOT,
    };

    use super::*;
    use crate::windows::hotkey::as_key_press;

    fn shortcut(modifiers: HOT_KEY_MODIFIERS, key: VIRTUAL_KEY) -> Option<Command> {
        let keys = ((key.0 as isize) << 16) | modifiers.0 as isize;
        command(as_key_press(LPARAM(keys)))
    }

    fn bare(key: VIRTUAL_KEY) -> Option<Command> {
        shortcut(HOT_KEY_MODIFIERS(0), key)
    }

    #[test]
    fn a_shortcut_on_a_pin_acts_as_the_pins_key_with_just_those_modifiers() {
        assert_eq!(shortcut(MOD_CONTROL, VK_C), Some(Command::Copy));
        assert_eq!(shortcut(MOD_CONTROL, VK_S), Some(Command::Save));
        assert_eq!(
            shortcut(MOD_CONTROL | MOD_SHIFT, VK_S),
            Some(Command::SaveAs)
        );
        assert_eq!(shortcut(MOD_CONTROL, VK_0), Some(Command::ActualSize));
        assert_eq!(shortcut(MOD_SHIFT, VK_F10), Some(Command::Menu));
        assert_eq!(bare(VK_LEFT), Some(Command::Nudge(-1, 0)));
        assert_eq!(shortcut(MOD_SHIFT, VK_DOWN), Some(Command::Nudge(0, 10)));
    }

    #[test]
    fn a_key_with_other_modifiers_is_no_pin_key_and_stays_a_shortcut() {
        assert_eq!(shortcut(MOD_CONTROL | MOD_SHIFT, VK_C), None);
        assert_eq!(shortcut(MOD_CONTROL | MOD_ALT, VK_S), None);
        assert_eq!(shortcut(MOD_CONTROL | MOD_ALT | MOD_SHIFT, VK_S), None);
        assert_eq!(shortcut(MOD_CONTROL | MOD_SHIFT, VK_0), None);
        assert_eq!(shortcut(MOD_ALT | MOD_SHIFT, VK_F10), None);
        assert_eq!(shortcut(MOD_CONTROL, VK_LEFT), None);
        assert_eq!(shortcut(MOD_CONTROL | MOD_SHIFT, VK_RIGHT), None);
        assert_eq!(shortcut(MOD_ALT, VK_UP), None);
        assert_eq!(bare(VK_C), None);
        assert_eq!(bare(VK_F10), None);
        assert_eq!(bare(VK_SNAPSHOT), None);
        assert_eq!(shortcut(MOD_CONTROL, VK_SNAPSHOT), None);
    }

    #[test]
    fn the_zoom_and_fade_keys_take_shift_as_a_plus_sign_needs_it() {
        assert_eq!(bare(VK_OEM_PLUS), Some(Command::Zoom(1)));
        assert_eq!(shortcut(MOD_SHIFT, VK_OEM_PLUS), Some(Command::Zoom(1)));
        assert_eq!(bare(VK_SUBTRACT), Some(Command::Zoom(-1)));
        assert_eq!(shortcut(MOD_CONTROL, VK_ADD), Some(Command::Fade(1)));
        assert_eq!(
            shortcut(MOD_CONTROL | MOD_SHIFT, VK_OEM_MINUS),
            Some(Command::Fade(-1))
        );
        assert_eq!(shortcut(MOD_ALT, VK_OEM_PLUS), None);
    }

    #[test]
    fn a_held_key_moves_and_zooms_again_but_closes_copies_and_saves_once() {
        let held = |key| KeyPress {
            repeat: true,
            ..as_key_press(LPARAM((key as isize) << 16))
        };
        assert_eq!(command(held(VK_ESCAPE.0)), None);
        assert_eq!(command(held(VK_APPS.0)), None);
        assert_eq!(command(held(VK_RIGHT.0)), Some(Command::Nudge(1, 0)));
        assert_eq!(command(held(VK_OEM_MINUS.0)), Some(Command::Zoom(-1)));
        let held_copy = KeyPress {
            control: true,
            ..held(VK_C.0)
        };
        assert_eq!(command(held_copy), None);
        assert_eq!(bare(VK_ESCAPE), Some(Command::Close));
    }
}
