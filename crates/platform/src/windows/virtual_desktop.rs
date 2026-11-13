//! Brings the app's windows to the virtual desktop the user is on when they were left on another.

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows::core::{Error, GUID, Result};

use super::window_list;

pub struct VirtualDesktops(IVirtualDesktopManager);

impl VirtualDesktops {
    pub fn new() -> Result<Self> {
        unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) }.map(Self)
    }

    // The first call took 12 ms on a press; making it at start keeps that off the hotkey.
    pub fn warm_up(&self, window: HWND) {
        let _ = unsafe { self.0.IsWindowOnCurrentVirtualDesktop(window) };
    }

    pub fn bring_here(&self, windows: &[HWND], here: HWND) -> Result<bool> {
        let Some(&first) = windows.first() else {
            return Ok(false);
        };
        if unsafe { self.0.IsWindowOnCurrentVirtualDesktop(first) }?.as_bool() {
            return Ok(false);
        }
        let desktop = self
            .desktop_of(here)
            .or_else(|| self.current_desktop())
            .ok_or_else(|| Error::new(E_FAIL, "the current virtual desktop is unknown"))?;
        for &window in windows {
            unsafe { self.0.MoveWindowToDesktop(window, &desktop) }?;
        }
        Ok(true)
    }

    fn desktop_of(&self, window: HWND) -> Option<GUID> {
        let desktop = unsafe { self.0.GetWindowDesktopId(window) }.ok()?;
        (desktop != GUID::zeroed()).then_some(desktop)
    }

    // Shell windows such as the wallpaper and the taskbar have no desktop id; app windows here do.
    fn current_desktop(&self) -> Option<GUID> {
        window_list::shown_windows()
            .into_iter()
            .filter(|&window| {
                unsafe { self.0.IsWindowOnCurrentVirtualDesktop(window) }
                    .is_ok_and(|here| here.as_bool())
            })
            .find_map(|window| self.desktop_of(window))
    }
}
