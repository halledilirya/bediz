//! Moving the controls bar with the mouse, kept within the work area of its monitor.

use std::cell::Cell;

use bediz_core::geometry::{Point, Rect};
use bediz_core::recording;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};

#[derive(Clone, Copy)]
struct Drag {
    window: HWND,
    offset: Point,
    width: i32,
    height: i32,
    work: Rect,
}

thread_local! {
    static DRAG: Cell<Option<Drag>> = const { Cell::new(None) };
}

pub fn begin(window: HWND) {
    let mut bounds = RECT::default();
    if unsafe { GetWindowRect(window, &mut bounds) }.is_err() {
        return;
    }
    let Some(cursor) = cursor() else {
        return;
    };
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return;
    }
    let work = info.rcWork;
    DRAG.with(|drag| {
        drag.set(Some(Drag {
            window,
            offset: Point::new(cursor.x - bounds.left, cursor.y - bounds.top),
            width: bounds.right - bounds.left,
            height: bounds.bottom - bounds.top,
            work: Rect::new(work.left, work.top, work.right, work.bottom),
        }));
    });
    unsafe { SetCapture(window) };
}

pub fn follow(window: HWND, button_down: bool) {
    let Some(drag) = DRAG.with(Cell::get).filter(|drag| drag.window == window) else {
        return;
    };
    if !button_down {
        end();
        return;
    }
    let Some(cursor) = cursor() else {
        return;
    };
    let wanted = Point::new(cursor.x - drag.offset.x, cursor.y - drag.offset.y);
    let at = recording::keep_within(wanted, drag.width, drag.height, drag.work);
    let _ = unsafe {
        SetWindowPos(
            window,
            None,
            at.x,
            at.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

pub fn end() {
    if let Some(drag) = DRAG.with(Cell::take)
        && unsafe { GetCapture() } == drag.window
    {
        let _ = unsafe { ReleaseCapture() };
    }
}

fn cursor() -> Option<POINT> {
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }.ok()?;
    Some(cursor)
}
