//! The pins' window procedure, which leaves what a pin's window hears for the runtime to pick up.

use std::cell::Cell;
use std::ffi::c_void;

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, MA_NOACTIVATE, RemovePropW, SetPropW, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE,
    WM_CREATE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCDESTROY,
    WM_PAINT, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE,
};
use windows::core::{PCWSTR, w};

use super::{COLOURS_CHANGED, DISPLAY_CHANGED, guarded};
use crate::windows::pin::PinEvent;

const NOT_FULL_SCREEN: PCWSTR = w!("NonRudeHWND");

thread_local! {
    static PIN_EVENTS: Cell<Vec<(HWND, PinEvent)>> = const { Cell::new(Vec::new()) };
}

pub fn take_pin_events() -> Vec<(HWND, PinEvent)> {
    PIN_EVENTS.with(Cell::take)
}

fn heard_from_pin(window: HWND, event: PinEvent) {
    PIN_EVENTS.with(|events| {
        let mut heard = events.take();
        heard.push((window, event));
        events.set(heard);
    });
}

pub(super) unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    guarded(LRESULT(0), || on_message(window, message, wparam, lparam))
}

fn on_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // Over a whole monitor a pin would pass for a full-screen app, with the taskbar under it.
        WM_CREATE => {
            let marked = HANDLE(std::ptr::without_provenance_mut::<c_void>(1));
            let _ = unsafe { SetPropW(window, NOT_FULL_SCREEN, Some(marked)) };
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_NCDESTROY => {
            let _ = unsafe { RemovePropW(window, NOT_FULL_SCREEN) };
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ACTIVATE => {
            let active = (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE;
            heard_from_pin(window, PinEvent::Activated(active));
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        // A pin keeps its pixels on every monitor; only its border follows the new scale.
        WM_DPICHANGED => {
            heard_from_pin(window, PinEvent::Rescaled);
            LRESULT(0)
        }
        // The runtime owns the pin, so Alt+F4 or a close asked through UI Automation is left to it.
        WM_CLOSE => {
            heard_from_pin(window, PinEvent::Close);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            DISPLAY_CHANGED.with(|flag| flag.set(true));
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
            COLOURS_CHANGED.with(|flag| flag.set(true));
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_ERASEBKGND => LRESULT(1),
        // DirectComposition paints every window whose device it lost, so a paint is when to check.
        WM_PAINT => {
            heard_from_pin(window, PinEvent::Painted);
            let _ = unsafe { ValidateRect(Some(window), None) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
