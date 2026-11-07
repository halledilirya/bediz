use bediz_core::geometry::{Point, Rect};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongW, IsIconic, IsWindowVisible,
    WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use super::window_class::guarded;

// The shell draws the wallpaper and the icons in these; they span every monitor.
const DESKTOP_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

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
        let rects = &mut self.rects;
        each_window(|window| {
            if let Some(rect) = selectable_bounds(window) {
                rects.push(rect);
            }
        });
        self.rects.len()
    }

    pub fn window_at(&self, at: Point) -> Option<Rect> {
        self.rects.iter().copied().find(|rect| rect.contains(at))
    }
}

pub fn shown_windows() -> Vec<HWND> {
    let mut shown = Vec::new();
    each_window(|window| {
        if is_shown(window) {
            shown.push(window);
        }
    });
    shown
}

fn each_window(mut visit: impl FnMut(HWND)) {
    let mut visit: &mut dyn FnMut(HWND) = &mut visit;
    let target = LPARAM(&mut visit as *mut &mut dyn FnMut(HWND) as isize);
    let _ = unsafe { EnumWindows(Some(visit_one), target) };
}

unsafe extern "system" fn visit_one(window: HWND, target: LPARAM) -> BOOL {
    let visit = unsafe { &mut *(target.0 as *mut &mut dyn FnMut(HWND)) };
    guarded(false, || {
        visit(window);
        true
    })
    .into()
}

fn is_shown(window: HWND) -> bool {
    if !unsafe { IsWindowVisible(window) }.as_bool() || unsafe { IsIconic(window) }.as_bool() {
        return false;
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
    !(cloaked_read.is_ok() && cloaked != 0)
}

fn selectable_bounds(window: HWND) -> Option<Rect> {
    let extended = unsafe { GetWindowLongW(window, GWL_EXSTYLE) } as u32;
    if extended & WS_EX_TRANSPARENT.0 != 0 || !is_shown(window) || is_desktop(window) {
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

fn is_desktop(window: HWND) -> bool {
    let mut class = [0u16; 16];
    let length = unsafe { GetClassNameW(window, &mut class) };
    let name = usize::try_from(length)
        .ok()
        .and_then(|length| class.get(..length))
        .map(String::from_utf16_lossy)
        .unwrap_or_default();
    DESKTOP_CLASSES.contains(&name.as_str())
}
