//! Notices when Windows switches desktops, such as to the lock screen or an elevation prompt.

use std::cell::Cell;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{EVENT_SYSTEM_DESKTOPSWITCH, WINEVENT_OUTOFCONTEXT};

use super::window_class::guarded;

thread_local! {
    static SWITCHED: Cell<bool> = const { Cell::new(false) };
}

pub struct DesktopSwitchHook(HWINEVENTHOOK);

impl DesktopSwitchHook {
    pub fn install() -> Option<Self> {
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_DESKTOPSWITCH,
                EVENT_SYSTEM_DESKTOPSWITCH,
                None,
                Some(switched),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        (!hook.is_invalid()).then_some(Self(hook))
    }
}

impl Drop for DesktopSwitchHook {
    fn drop(&mut self) {
        let _ = unsafe { UnhookWinEvent(self.0) };
    }
}

pub fn take_switched() -> bool {
    SWITCHED.with(|flag| flag.replace(false))
}

unsafe extern "system" fn switched(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    guarded((), || SWITCHED.with(|flag| flag.set(true)));
}
