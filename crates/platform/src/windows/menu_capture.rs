//! Keeps a menu out of screenshots, recordings and screen sharing while it is open, as the window
//! that owns it is kept out of them.

use std::cell::Cell;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CWPSTRUCT, CallNextHookEx, HC_ACTION, HHOOK, SetWindowDisplayAffinity, SetWindowsHookExW,
    UnhookWindowsHookEx, WDA_EXCLUDEFROMCAPTURE, WH_CALLWNDPROC, WM_CREATE,
};
use windows::core::Error;

use super::window_class::guarded;
use super::window_list::class_name;

// The system makes a menu and its shadow as windows it never hands over, on the thread tracking it.
const MENU_CLASSES: [&str; 2] = ["#32768", "SysShadow"];

thread_local! {
    static MISSED: Cell<Option<Error>> = const { Cell::new(None) };
}

struct Hook(HHOOK);

impl Hook {
    fn missed(self) -> Option<Error> {
        MISSED.with(Cell::take)
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        let _ = unsafe { UnhookWindowsHookEx(self.0) };
        MISSED.with(|missed| missed.set(None));
    }
}

pub fn kept_out_while<T>(kept_out: bool, open: impl FnOnce() -> T) -> (T, Option<String>) {
    if !kept_out {
        return (open(), None);
    }
    let thread = unsafe { GetCurrentThreadId() };
    let hook = match unsafe { SetWindowsHookExW(WH_CALLWNDPROC, Some(on_sent), None, thread) } {
        Ok(hook) => Hook(hook),
        Err(error) => {
            let note = format!("a menu could not be kept out of captures: {error}");
            return (open(), Some(note));
        }
    };
    let shown = open();
    let note = hook
        .missed()
        .map(|error| format!("a menu's window could not be kept out of captures: {error}"));
    (shown, note)
}

unsafe extern "system" fn on_sent(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if u32::try_from(code) == Ok(HC_ACTION) {
        guarded((), || {
            let sent = unsafe { (lparam.0 as *const CWPSTRUCT).as_ref() };
            if let Some(sent) = sent
                && sent.message == WM_CREATE
                && MENU_CLASSES.contains(&class_name(sent.hwnd).as_str())
                && let Err(error) =
                    unsafe { SetWindowDisplayAffinity(sent.hwnd, WDA_EXCLUDEFROMCAPTURE) }
            {
                MISSED.with(|missed| {
                    let first = missed.take();
                    missed.set(first.or(Some(error)));
                });
            }
        });
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::E_ACCESSDENIED;

    use super::*;

    #[test]
    fn a_menu_window_left_in_captures_is_told_once_and_only_while_kept_out() {
        let (shown, note) = kept_out_while(true, || {
            MISSED.with(|missed| missed.set(Some(Error::from_hresult(E_ACCESSDENIED))));
            "shown"
        });
        assert_eq!(shown, "shown");
        assert!(note.is_some_and(|note| note.starts_with("a menu's window")));
        assert_eq!(kept_out_while(true, || 7), (7, None));
        assert_eq!(kept_out_while(false, || 8), (8, None));
    }

    #[test]
    fn nothing_missed_outlives_a_menu_that_panics() {
        let unwound: std::thread::Result<((), Option<String>)> = std::panic::catch_unwind(|| {
            kept_out_while(true, || {
                MISSED.with(|missed| missed.set(Some(Error::from_hresult(E_ACCESSDENIED))));
                panic!("the menu failed")
            })
        });
        assert!(unwound.is_err());
        assert!(MISSED.with(Cell::take).is_none());
    }
}
