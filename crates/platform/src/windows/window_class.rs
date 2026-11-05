use std::cell::Cell;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, DefWindowProcW, EndMenu, HCURSOR, HTCLIENT, IDC_CROSS, LoadCursorW,
    RegisterClassExW, SetCursor, WM_APP, WM_CLOSE, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_PAINT,
    WM_SETCURSOR, WNDCLASSEXW,
};
use windows::core::{Error, PCWSTR, Result, w};

use super::runtime::WM_STOP;

pub const MESSAGE_CLASS: PCWSTR = w!("BedizMessage");
pub const OVERLAY_CLASS: PCWSTR = w!("BedizOverlay");
pub const LABEL_CLASS: PCWSTR = w!("BedizLabel");

thread_local! {
    static DISPLAY_CHANGED: Cell<bool> = const { Cell::new(false) };
    static CLOSE_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static CURSOR: Cell<Option<HCURSOR>> = const { Cell::new(None) };
    static MISSED: Cell<Vec<u32>> = const { Cell::new(Vec::new()) };
}

pub fn take_missed() -> Vec<u32> {
    MISSED.with(Cell::take)
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

unsafe extern "system" fn message_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

unsafe extern "system" fn label_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

unsafe extern "system" fn overlay_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_DISPLAYCHANGE => {
            DISPLAY_CHANGED.with(|flag| flag.set(true));
            LRESULT(0)
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
