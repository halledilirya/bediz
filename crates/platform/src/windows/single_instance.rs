use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, WPARAM,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW,
};
use windows::core::{PCWSTR, w};

use super::runtime::WM_SECOND_START;
use super::window_class::MESSAGE_CLASS;

pub struct FirstInstance(Option<HANDLE>);

impl Drop for FirstInstance {
    fn drop(&mut self) {
        if let Some(handle) = self.0 {
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}

pub fn first_instance() -> Option<FirstInstance> {
    let mutex = unsafe { CreateMutexW(None, false, w!("Local\\io.github.halledilirya.bediz")) };
    let running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    match mutex {
        Ok(handle) if running => {
            let _ = unsafe { CloseHandle(handle) };
            wake_the_first();
            None
        }
        Ok(handle) => Some(FirstInstance(Some(handle))),
        Err(_) => Some(FirstInstance(None)),
    }
}

fn wake_the_first() {
    let found = unsafe { FindWindowExW(Some(HWND_MESSAGE), None, MESSAGE_CLASS, PCWSTR::null()) };
    let Ok(window) = found else {
        return;
    };
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
    // Only the foreground process can let another one bring its settings window to the front.
    let _ = unsafe { AllowSetForegroundWindow(process) };
    let _ = unsafe { PostMessageW(Some(window), WM_SECOND_START, WPARAM(0), LPARAM(0)) };
}
