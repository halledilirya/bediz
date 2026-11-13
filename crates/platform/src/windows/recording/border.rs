//! The frame around the recorded area, kept out of the recording: solid while recording, dashed
//! around an area locked before it starts, and in the system's colours under high contrast.

use bediz_core::geometry::Rect;
use bediz_core::recording;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, CreateSolidBrush, DeleteObject, FillRect, GetSysColor,
    HDC,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetClientRect, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos,
    USER_DEFAULT_SCREEN_DPI, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Result, w};

use crate::windows::contrast::high_contrast;
use crate::windows::window_class::{BORDER_CLASS, DASHED_BORDER_CLASS, OwnedWindow};

const THICKNESS: f32 = 2.0;
const DASH: f32 = 6.0;
const COLOUR: COLORREF = COLORREF(0x0023_11E8);
const LOCKED_COLOUR: COLORREF = COLORREF(0x0000_C8FF);
const LOCKED_GAP: COLORREF = COLORREF(0x0020_2020);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Solid,
    Dashed,
}

pub fn erase(window: HWND, dc: HDC, line: Line) {
    let mut client = RECT::default();
    if unsafe { GetClientRect(window, &mut client) }.is_err() {
        return;
    }
    let contrast = high_contrast();
    let system = |index| COLORREF(unsafe { GetSysColor(index) });
    let (ink, gap) = match (line, contrast) {
        (Line::Solid, false) => (COLOUR, COLOUR),
        (Line::Solid, true) => (system(COLOR_HIGHLIGHT), system(COLOR_HIGHLIGHT)),
        (Line::Dashed, false) => (LOCKED_COLOUR, LOCKED_GAP),
        (Line::Dashed, true) => (system(COLOR_HIGHLIGHT), system(COLOR_HIGHLIGHTTEXT)),
    };
    fill(dc, &client, gap);
    if line == Line::Solid {
        return;
    }
    let scale = unsafe { GetDpiForWindow(window) } as f32 / USER_DEFAULT_SCREEN_DPI as f32;
    let dash = (DASH * scale).round() as i32;
    let across = client.right >= client.bottom;
    let length = if across { client.right } else { client.bottom };
    for (start, end) in recording::dashes(length, dash) {
        let part = if across {
            RECT {
                left: start,
                right: end,
                ..client
            }
        } else {
            RECT {
                top: start,
                bottom: end,
                ..client
            }
        };
        fill(dc, &part, ink);
    }
}

fn fill(dc: HDC, rect: &RECT, colour: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(colour);
        FillRect(dc, rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

pub struct Border {
    windows: Vec<OwnedWindow>,
}

impl Border {
    pub fn create(
        instance: HINSTANCE,
        area: Rect,
        monitor: Rect,
        scale: f32,
        line: Line,
    ) -> Result<Self> {
        let thickness = ((THICKNESS * scale).round() as i32).max(1);
        let class = match line {
            Line::Solid => BORDER_CLASS,
            Line::Dashed => DASHED_BORDER_CLASS,
        };
        let mut windows = Vec::with_capacity(4);
        for side in recording::border(area, monitor, thickness) {
            let window = OwnedWindow(unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    class,
                    w!(""),
                    WS_POPUP,
                    side.left,
                    side.top,
                    i32::try_from(side.width()).unwrap_or(0),
                    i32::try_from(side.height()).unwrap_or(0),
                    None,
                    None,
                    Some(instance),
                    None,
                )
            }?);
            unsafe {
                SetLayeredWindowAttributes(window.0, COLORREF(0), u8::MAX, LWA_ALPHA)?;
                SetWindowDisplayAffinity(window.0, WDA_EXCLUDEFROMCAPTURE)?;
            }
            windows.push(window);
        }
        Ok(Self { windows })
    }

    pub fn reveal(&self) -> Result<()> {
        for window in &self.windows {
            unsafe {
                SetWindowPos(
                    window.0,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
            }?;
        }
        Ok(())
    }
}
