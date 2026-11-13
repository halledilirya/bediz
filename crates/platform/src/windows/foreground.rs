//! Takes the foreground for one of the app's windows and gives it back to the window that had it.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_MOUSE, SendInput, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foreground {
    Taken,
    TakenAfterInput,
    Refused,
}

pub fn take(window: HWND) -> Foreground {
    let taken = if unsafe { SetForegroundWindow(window) }.as_bool() {
        Foreground::Taken
    } else if take_after_input(window) {
        Foreground::TakenAfterInput
    } else {
        Foreground::Refused
    };
    let _ = unsafe { SetFocus(Some(window)) };
    taken
}

pub fn give_back(previous: HWND) {
    if unsafe { IsWindow(Some(previous)) }.as_bool() {
        let _ = unsafe { SetForegroundWindow(previous) };
    }
}

pub fn give_back_from_us(previous: HWND) {
    let current = unsafe { GetForegroundWindow() };
    let mut process = 0;
    if !current.is_invalid() {
        unsafe { GetWindowThreadProcessId(current, Some(&mut process)) };
    }
    if current.is_invalid() || process == unsafe { GetCurrentProcessId() } {
        give_back(previous);
    }
}

// Windows can refuse the foreground even to the hotkey's owner; an empty input lifts that, as in PowerToys.
fn take_after_input(window: HWND) -> bool {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        ..Default::default()
    };
    let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
    sent == 1 && unsafe { SetForegroundWindow(window) }.as_bool()
}
