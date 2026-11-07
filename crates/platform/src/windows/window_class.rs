use std::cell::Cell;
use std::panic::AssertUnwindSafe;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, DefWindowProcW, DestroyWindow, EndMenu, HCURSOR, HTCLIENT, IDC_CROSS, LoadCursorW,
    RegisterClassExW, SetCursor, WM_APP, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND,
    WM_GETOBJECT, WM_HOTKEY, WM_PAINT, WM_SETCURSOR, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE,
    WNDCLASSEXW,
};
use windows::core::{Error, PCWSTR, Result, w};

use super::hotkey::HOTKEY;
use super::label_window::automation;
use super::runtime::WM_STOP;

pub const MESSAGE_CLASS: PCWSTR = w!("BedizMessage");
pub const OVERLAY_CLASS: PCWSTR = w!("BedizOverlay");
pub const LABEL_CLASS: PCWSTR = w!("BedizLabel");

thread_local! {
    static DISPLAY_CHANGED: Cell<bool> = const { Cell::new(false) };
    static CLOSE_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static LABEL_CLOSE_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static COLOURS_CHANGED: Cell<bool> = const { Cell::new(false) };
    static CURSOR: Cell<Option<HCURSOR>> = const { Cell::new(None) };
    static MISSED: Cell<Vec<u32>> = const { Cell::new(Vec::new()) };
    static MISSED_HOTKEY: Cell<Option<LPARAM>> = const { Cell::new(None) };
}

pub struct OwnedWindow(pub HWND);

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

pub fn take_missed() -> Vec<u32> {
    MISSED.with(Cell::take)
}

pub fn take_missed_hotkey() -> Option<LPARAM> {
    MISSED_HOTKEY.with(Cell::take)
}

pub fn use_cursor(cursor: HCURSOR) {
    CURSOR.with(|current| current.set(Some(cursor)));
    unsafe { SetCursor(Some(cursor)) };
}

pub fn take_display_changed() -> bool {
    DISPLAY_CHANGED.with(|flag| flag.replace(false))
}

pub fn take_close_requested() -> bool {
    CLOSE_REQUESTED.with(|flag| flag.replace(false))
}

pub fn take_label_close_requested() -> bool {
    LABEL_CLOSE_REQUESTED.with(|flag| flag.replace(false))
}

pub fn take_colours_changed() -> bool {
    COLOURS_CHANGED.with(|flag| flag.replace(false))
}

pub fn register(instance: HINSTANCE) -> Result<()> {
    let message = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(message_procedure),
        hInstance: instance,
        lpszClassName: MESSAGE_CLASS,
        ..Default::default()
    };
    register_class(&message)?;
    let overlay = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_DBLCLKS,
        lpfnWndProc: Some(overlay_procedure),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_CROSS) }?,
        lpszClassName: OVERLAY_CLASS,
        ..Default::default()
    };
    register_class(&overlay)?;
    let label = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(label_procedure),
        hInstance: instance,
        lpszClassName: LABEL_CLASS,
        ..Default::default()
    };
    register_class(&label)
}

fn register_class(class: &WNDCLASSEXW) -> Result<()> {
    if unsafe { RegisterClassExW(class) } != 0 {
        return Ok(());
    }
    let error = Error::from_thread();
    if error.code() == ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
        return Ok(());
    }
    Err(error)
}

// A panic leaving an extern "system" function aborts the process, which would take the hotkey with it.
pub fn guarded<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

unsafe extern "system" fn message_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    guarded(LRESULT(0), || on_message(window, message, wparam, lparam))
}

fn on_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // The runtime's pump handles its own messages; a modal loop (a menu, a dialog) dispatches them here.
    if message >= WM_APP {
        if message == WM_STOP {
            let _ = unsafe { EndMenu() };
        }
        MISSED.with(|missed| {
            let mut messages = missed.take();
            if !messages.contains(&message) {
                messages.push(message);
            }
            missed.set(messages);
        });
        return LRESULT(0);
    }
    if message == WM_HOTKEY && wparam.0 == HOTKEY as usize {
        MISSED_HOTKEY.with(|missed| missed.set(Some(lparam)));
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

unsafe extern "system" fn label_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    guarded(LRESULT(0), || {
        match message {
            // Without overlays the label is the only window that hears of a display change.
            WM_DISPLAYCHANGE => DISPLAY_CHANGED.with(|flag| flag.set(true)),
            WM_GETOBJECT => {
                if let Some(answer) = automation::answer(window, wparam, lparam) {
                    return answer;
                }
            }
            // The runtime owns the label, so a close asked through UI Automation is left to it.
            WM_CLOSE => {
                LABEL_CLOSE_REQUESTED.with(|flag| flag.set(true));
                return LRESULT(0);
            }
            WM_DESTROY => automation::detach(window),
            _ => {}
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    })
}

unsafe extern "system" fn overlay_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    guarded(LRESULT(0), || {
        on_overlay_message(window, message, wparam, lparam)
    })
}

fn on_overlay_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_DISPLAYCHANGE => {
            DISPLAY_CHANGED.with(|flag| flag.set(true));
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
            COLOURS_CHANGED.with(|flag| flag.set(true));
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_CLOSE => {
            CLOSE_REQUESTED.with(|flag| flag.set(true));
            LRESULT(0)
        }
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => match CURSOR.with(Cell::get) {
            Some(cursor) => {
                unsafe { SetCursor(Some(cursor)) };
                LRESULT(1)
            }
            None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        },
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let _ = unsafe { ValidateRect(Some(window), None) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
