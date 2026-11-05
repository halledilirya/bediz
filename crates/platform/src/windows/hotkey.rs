use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_SNAPSHOT,
};
use windows::core::{Error, Result};

use super::print_screen_setting::{self, Watch};
use crate::event::HotkeyState;

pub const PRINT_SCREEN: i32 = 1;

pub struct Change {
    pub state: HotkeyState,
    pub note: String,
}

pub struct PrintScreen {
    window: HWND,
    registered: bool,
    arrived: bool,
    state: Option<HotkeyState>,
    watch: Option<Watch>,
}

impl PrintScreen {
    pub fn new(window: HWND) -> Self {
        Self {
            window,
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

    pub fn arrived(&mut self) -> Option<Change> {
        if self.arrived {
            return None;
        }
        self.arrived = true;
        self.evaluate(None)
    }

    pub fn refresh(&mut self) -> Option<Change> {
        let error = if self.registered {
            None
        } else {
            self.register().err()
        };
        self.evaluate(error)
    }

    pub fn unregister(&mut self) {
        if std::mem::take(&mut self.registered) {
            let _ = unsafe { UnregisterHotKey(Some(self.window), PRINT_SCREEN) };
        }
        self.arrived = false;
        self.state = None;
    }

    fn register(&mut self) -> Result<()> {
        unsafe {
            RegisterHotKey(
                Some(self.window),
                PRINT_SCREEN,
                MOD_NOREPEAT,
                u32::from(VK_SNAPSHOT.0),
            )
        }?;
        self.registered = true;
        Ok(())
    }

    fn evaluate(&mut self, error: Option<Error>) -> Option<Change> {
        let setting = print_screen_setting::read();
        let state = state_for(setting, self.registered, self.arrived);
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
                "print-screen registered={} arrived={} snipping_tool_setting={setting}{error}",
                self.registered, self.arrived
            ),
        })
    }
}

fn state_for(snipping_tool_setting: Option<u32>, registered: bool, arrived: bool) -> HotkeyState {
    match (snipping_tool_setting, registered) {
        (_, true) if arrived => HotkeyState::Registered,
        (Some(1), _) => HotkeyState::SnippingTool,
        (_, true) => HotkeyState::Registered,
        (_, false) => HotkeyState::Taken,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snipping_tool_setting_wins_until_the_key_arrives() {
        assert_eq!(state_for(Some(1), true, false), HotkeyState::SnippingTool);
        assert_eq!(state_for(Some(1), false, false), HotkeyState::SnippingTool);
        assert_eq!(state_for(Some(1), true, true), HotkeyState::Registered);
    }

    #[test]
    fn without_the_snipping_tool_the_registration_decides() {
        for setting in [None, Some(0), Some(2)] {
            for arrived in [false, true] {
                assert_eq!(state_for(setting, true, arrived), HotkeyState::Registered);
                assert_eq!(state_for(setting, false, arrived), HotkeyState::Taken);
            }
        }
    }
}
