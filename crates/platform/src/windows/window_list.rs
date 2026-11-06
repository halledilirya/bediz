use bediz_core::geometry::{Point, Rect};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetWindowLongW, IsIconic, IsWindowVisible, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use super::window_class::guarded;

pub struct WindowList {
    rects: Vec<Rect>,
}

impl WindowList {
    pub fn new() -> Self {
        Self {
            rects: Vec::with_capacity(256),
        }
    }

    pub fn refresh(&mut self) -> usize {
        self.rects.clear();
        let target = LPARAM(&mut self.rects as *mut Vec<Rect> as isize);
        let _ = unsafe { EnumWindows(Some(collect), target) };
        self.rects.len()
    }

    pub fn window_at(&self, at: Point) -> Option<Rect> {
        self.rects.iter().copied().find(|rect| rect.contains(at))
    }
}

unsafe extern "system" fn collect(window: HWND, target: LPARAM) -> BOOL {
    let rects = unsafe { &mut *(target.0 as *mut Vec<Rect>) };
    guarded(false, || {
        if let Some(rect) = selectable_bounds(window) {
            rects.push(rect);
        }
        true
    })
    .into()
}

fn selectable_bounds(window: HWND) -> Option<Rect> {
    let shown =
        unsafe { IsWindowVisible(window) }.as_bool() && !unsafe { IsIconic(window) }.as_bool();
    let extended = unsafe { GetWindowLongW(window, GWL_EXSTYLE) } as u32;
    if !shown || extended & WS_EX_TRANSPARENT.0 != 0 {
        return None;
    }
    let mut cloaked = 0u32;
    let cloaked_read = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        )
    };
    if cloaked_read.is_ok() && cloaked != 0 {
        return None;
    }
    let mut bounds = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()?;
    let rect = Rect::new(bounds.left, bounds.top, bounds.right, bounds.bottom);
    (!rect.is_empty()).then_some(rect)
}
