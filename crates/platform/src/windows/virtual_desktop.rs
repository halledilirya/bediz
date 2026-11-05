use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows::core::{Error, GUID, Result};

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
        let desktop = unsafe { self.0.GetWindowDesktopId(here) }?;
        if desktop == GUID::zeroed() {
            return Err(Error::new(E_FAIL, "the current virtual desktop is unknown"));
        }
        for &window in windows {
            unsafe { self.0.MoveWindowToDesktop(window, &desktop) }?;
        }
        Ok(true)
    }
}
