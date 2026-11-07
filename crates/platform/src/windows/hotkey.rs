use bediz_core::hotkey::{Hotkey, Key};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
    UnregisterHotKey, VIRTUAL_KEY, VK_0, VK_A, VK_DELETE, VK_DOWN, VK_END, VK_F1, VK_HOME,
    VK_INSERT, VK_LEFT, VK_NEXT, VK_PAUSE, VK_PRIOR, VK_RIGHT, VK_SCROLL, VK_SNAPSHOT, VK_SPACE,
    VK_UP,
};
use windows::core::{Error, Result};

use super::print_screen_setting::{self, Watch};
use super::session::KeyPress;
use crate::event::HotkeyState;

pub const HOTKEY: i32 = 1;

pub struct Change {
    pub state: HotkeyState,
    pub note: String,
}

pub struct Registration {
    window: HWND,
    hotkey: Option<Hotkey>,
    yields: bool,
    registered: bool,
    arrived: bool,
    state: Option<HotkeyState>,
    watch: Option<Watch>,
}

impl Registration {
    pub fn new(window: HWND) -> Self {
        Self {
            window,
            hotkey: None,
            yields: false,
            registered: false,
            arrived: false,
            state: None,
            watch: None,
        }
    }

    pub fn start_watching(&mut self, notes: &mut Vec<String>) {
        match Watch::start() {
            Ok(watch) => self.watch = Some(watch),
            Err(error) => notes.push(format!(
                "the Print Screen setting is not watched, so freeing the key needs a restart: {error}"
            )),
        }
    }

    pub fn wake_event(&self) -> Option<HANDLE> {
        self.watch.as_ref().map(Watch::event)
    }

    pub fn set(&mut self, hotkey: Hotkey, yields: bool) -> Option<Change> {
        if self.hotkey == Some(hotkey) {
            if std::mem::replace(&mut self.yields, yields) == yields {
                return None;
            }
            return self.evaluate(None);
        }
        self.unregister();
        self.hotkey = Some(hotkey);
        self.yields = yields;
        self.refresh()
    }

    pub fn setting_changed(&mut self, notes: &mut Vec<String>) -> Option<Change> {
        if let Some(watch) = &self.watch
            && let Err(error) = watch.arm()
        {
            notes.push(format!(
                "the Print Screen setting is no longer watched: {error}"
            ));
            self.watch = None;
        }
        self.arrived = false;
        self.refresh()
    }

    pub fn stop_watching(&mut self) {
        self.watch = None;
    }

    pub fn take_from_snipping_tool(&mut self, notes: &mut Vec<String>) -> Option<Change> {
        if let Err(error) = print_screen_setting::turn_off_snipping_tool() {
            notes.push(format!(
                "the Snipping Tool's Print Screen setting could not be turned off: {error}"
            ));
        }
        self.arrived = false;
        self.refresh()
    }

    pub fn arrived(&mut self) -> (Option<Change>, bool) {
        if self.arrived {
            return (None, false);
        }
        self.arrived = true;
        let proves_yielding = !self.yields
            && self.hotkey == Some(Hotkey::PRINT_SCREEN)
            && print_screen_setting::read() == Some(1);
        (self.evaluate(None), proves_yielding)
    }

    pub fn refresh(&mut self) -> Option<Change> {
        let hotkey = self.hotkey?;
        let error = if self.registered {
            None
        } else {
            self.register(hotkey).err()
        };
        self.evaluate(error)
    }

    pub fn unregister(&mut self) {
        if std::mem::take(&mut self.registered) {
            let _ = unsafe { UnregisterHotKey(Some(self.window), HOTKEY) };
        }
        self.arrived = false;
        self.state = None;
    }

    fn register(&mut self, hotkey: Hotkey) -> Result<()> {
        unsafe {
            RegisterHotKey(
                Some(self.window),
                HOTKEY,
                modifiers(hotkey),
                u32::from(virtual_key(hotkey.key).0),
            )
        }?;
        self.registered = true;
        Ok(())
    }

    fn evaluate(&mut self, error: Option<Error>) -> Option<Change> {
        let hotkey = self.hotkey?;
        let setting = print_screen_setting::read();
        let snipping_tool = hotkey == Hotkey::PRINT_SCREEN && setting == Some(1);
        let state = state_for(snipping_tool, self.yields, self.registered, self.arrived);
        if self.state == Some(state) {
            return None;
        }
        self.state = Some(state);
        let setting = setting.map_or_else(|| "absent".to_owned(), |value| value.to_string());
        let error = error.map_or_else(String::new, |error| {
            format!(" error={:#010x}", error.code().0)
        });
        Some(Change {
            state,
            note: format!(
                "hotkey {hotkey} registered={} arrived={} yields={} snipping_tool_setting={setting}{error}",
                self.registered, self.arrived, self.yields
            ),
        })
    }
}

fn state_for(snipping_tool: bool, yields: bool, registered: bool, arrived: bool) -> HotkeyState {
    match (registered, snipping_tool) {
        (true, false) => HotkeyState::Registered,
        (true, true) if arrived || yields => HotkeyState::Registered,
        (false, true) if !yields => HotkeyState::SnippingTool,
        (true, true) => HotkeyState::SnippingTool,
        (false, _) => HotkeyState::Taken,
    }
}

pub fn as_key_press(keys: LPARAM) -> KeyPress {
    let modifiers = keys.0 & 0xFFFF;
    let held = |modifier: HOT_KEY_MODIFIERS| modifiers & modifier.0 as isize != 0;
    KeyPress {
        key: VIRTUAL_KEY(((keys.0 >> 16) & 0xFFFF) as u16),
        alt: held(MOD_ALT),
        control: held(MOD_CONTROL),
        shift: held(MOD_SHIFT),
    }
}

fn modifiers(hotkey: Hotkey) -> HOT_KEY_MODIFIERS {
    let mut modifiers = MOD_NOREPEAT;
    if hotkey.ctrl {
        modifiers |= MOD_CONTROL;
    }
    if hotkey.alt {
        modifiers |= MOD_ALT;
    }
    if hotkey.shift {
        modifiers |= MOD_SHIFT;
    }
    modifiers
}

fn virtual_key(key: Key) -> VIRTUAL_KEY {
    match key {
        Key::Letter(letter) => VIRTUAL_KEY(VK_A.0 + (letter as u16).saturating_sub('A' as u16)),
        Key::Digit(digit) => VIRTUAL_KEY(VK_0.0 + u16::from(digit)),
        Key::Function(number) => VIRTUAL_KEY(VK_F1.0 + u16::from(number).saturating_sub(1)),
        Key::PrintScreen => VK_SNAPSHOT,
        Key::Pause => VK_PAUSE,
        Key::ScrollLock => VK_SCROLL,
        Key::Insert => VK_INSERT,
        Key::Delete => VK_DELETE,
        Key::Home => VK_HOME,
        Key::End => VK_END,
        Key::PageUp => VK_PRIOR,
        Key::PageDown => VK_NEXT,
        Key::Up => VK_UP,
        Key::Down => VK_DOWN,
        Key::Left => VK_LEFT,
        Key::Right => VK_RIGHT,
        Key::Space => VK_SPACE,
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_9, VK_F4, VK_F24, VK_S, VK_Z};

    use super::*;

    #[test]
    fn the_snipping_tool_setting_wins_until_the_key_arrives() {
        assert_eq!(
            state_for(true, false, true, false),
            HotkeyState::SnippingTool
        );
        assert_eq!(
            state_for(true, false, false, false),
            HotkeyState::SnippingTool
        );
        assert_eq!(state_for(true, false, true, true), HotkeyState::Registered);
    }

    #[test]
    fn a_build_seen_yielding_the_key_needs_no_press() {
        assert_eq!(state_for(true, true, true, false), HotkeyState::Registered);
        assert_eq!(state_for(true, true, false, false), HotkeyState::Taken);
    }

    #[test]
    fn without_the_snipping_tool_the_registration_decides() {
        for yields in [false, true] {
            for arrived in [false, true] {
                assert_eq!(
                    state_for(false, yields, true, arrived),
                    HotkeyState::Registered
                );
                assert_eq!(state_for(false, yields, false, arrived), HotkeyState::Taken);
            }
        }
    }

    #[test]
    fn keys_map_to_their_virtual_keys() {
        assert_eq!(virtual_key(Key::Letter('A')), VK_A);
        assert_eq!(virtual_key(Key::Letter('Z')), VK_Z);
        assert_eq!(virtual_key(Key::Digit(0)), VK_0);
        assert_eq!(virtual_key(Key::Digit(9)), VK_9);
        assert_eq!(virtual_key(Key::Function(1)), VK_F1);
        assert_eq!(virtual_key(Key::Function(24)), VK_F24);
        assert_eq!(virtual_key(Key::PrintScreen), VK_SNAPSHOT);
        assert_eq!(virtual_key(Key::PageUp), VK_PRIOR);
    }

    #[test]
    fn the_shortcut_pressed_on_the_overlay_becomes_that_key_with_its_modifiers() {
        let held = |modifiers: HOT_KEY_MODIFIERS, key: VIRTUAL_KEY| {
            LPARAM(((key.0 as isize) << 16) | modifiers.0 as isize)
        };
        let press = |key, alt, control, shift| KeyPress {
            key,
            alt,
            control,
            shift,
        };
        assert_eq!(
            as_key_press(held(MOD_CONTROL, VK_S)),
            press(VK_S, false, true, false)
        );
        assert_eq!(
            as_key_press(held(MOD_CONTROL | MOD_SHIFT, VK_S)),
            press(VK_S, false, true, true)
        );
        assert_eq!(
            as_key_press(held(MOD_ALT, VK_F4)),
            press(VK_F4, true, false, false)
        );
        assert_eq!(
            as_key_press(held(HOT_KEY_MODIFIERS(0), VK_SNAPSHOT)),
            press(VK_SNAPSHOT, false, false, false)
        );
    }

    #[test]
    fn modifiers_are_held_and_never_repeat() {
        let hotkey = Hotkey {
            ctrl: true,
            alt: false,
            shift: true,
            key: Key::Letter('A'),
        };
        assert_eq!(modifiers(hotkey), MOD_NOREPEAT | MOD_CONTROL | MOD_SHIFT);
        assert_eq!(modifiers(Hotkey::PRINT_SCREEN), MOD_NOREPEAT);
    }
}
