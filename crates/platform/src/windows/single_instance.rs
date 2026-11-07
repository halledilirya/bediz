use std::time::Duration;

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

const LOOKS: u32 = 100;
const BETWEEN_LOOKS: Duration = Duration::from_millis(100);

pub struct FirstInstance(Option<HANDLE>);

impl Drop for FirstInstance {
    fn drop(&mut self) {
        if let Some(handle) = self.0 {
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}

enum Claim<T> {
    Ours(T),
    Taken,
}

// The first start holds the name before its window exists, or may be quitting and free it.
pub fn first_instance() -> Option<FirstInstance> {
    first_or_woken(LOOKS, claim, wake_the_first, || {
        std::thread::sleep(BETWEEN_LOOKS);
    })
}

fn first_or_woken<T>(
    looks: u32,
    mut claim: impl FnMut() -> Claim<T>,
    mut wake: impl FnMut() -> bool,
    mut pause: impl FnMut(),
) -> Option<T> {
    for look in 1..=looks {
        match claim() {
            Claim::Ours(first) => return Some(first),
            Claim::Taken if wake() => return None,
            Claim::Taken if look < looks => pause(),
            Claim::Taken => {}
        }
    }
    None
}

fn claim() -> Claim<FirstInstance> {
    let mutex = unsafe { CreateMutexW(None, false, w!("Local\\io.github.halledilirya.bediz")) };
    let running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    match mutex {
        Ok(handle) if running => {
            let _ = unsafe { CloseHandle(handle) };
            Claim::Taken
        }
        Ok(handle) => Claim::Ours(FirstInstance(Some(handle))),
        Err(_) => Claim::Ours(FirstInstance(None)),
    }
}

fn wake_the_first() -> bool {
    let found = unsafe { FindWindowExW(Some(HWND_MESSAGE), None, MESSAGE_CLASS, PCWSTR::null()) };
    let Ok(window) = found else {
        return false;
    };
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
    // Only the foreground process can let another one bring its settings window to the front.
    let _ = unsafe { AllowSetForegroundWindow(process) };
    unsafe { PostMessageW(Some(window), WM_SECOND_START, WPARAM(0), LPARAM(0)) }.is_ok()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{Claim, first_or_woken};

    fn run(claims: &[bool], wakes: &[bool], looks: u32) -> (Option<u32>, u32, u32) {
        let (claimed, woken, paused) = (Cell::new(0), Cell::new(0), Cell::new(0));
        let first = first_or_woken(
            looks,
            || {
                let look = claimed.get();
                claimed.set(look + 1);
                match claims.get(look as usize) {
                    Some(true) => Claim::Ours(look),
                    _ => Claim::Taken,
                }
            },
            || {
                let look = woken.get();
                woken.set(look + 1);
                wakes.get(look as usize).copied().unwrap_or(false)
            },
            || paused.set(paused.get() + 1),
        );
        (first, claimed.get(), paused.get())
    }

    #[test]
    fn the_first_start_takes_the_name_at_once() {
        assert_eq!(run(&[true], &[], 100), (Some(0), 1, 0));
    }

    #[test]
    fn a_second_start_waits_for_the_first_ones_window() {
        assert_eq!(run(&[], &[false, false, true], 100), (None, 3, 2));
    }

    #[test]
    fn a_second_start_takes_over_when_the_first_one_has_quit() {
        assert_eq!(run(&[false, false, true], &[], 100), (Some(2), 3, 2));
    }

    #[test]
    fn a_second_start_gives_up_after_its_last_look() {
        assert_eq!(run(&[], &[], 5), (None, 5, 4));
    }
}
