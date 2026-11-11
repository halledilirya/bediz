use bediz_core::geometry::Rect;
use bediz_core::recording;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    COLOR_HIGHLIGHT, CreateSolidBrush, DeleteObject, FillRect, GetSysColorBrush, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetClientRect, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos,
    WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Result, w};

use crate::windows::contrast::high_contrast;
use crate::windows::window_class::{BORDER_CLASS, OwnedWindow};

const THICKNESS: f32 = 2.0;
const COLOUR: COLORREF = COLORREF(0x0023_11E8);

pub fn erase(window: HWND, dc: HDC) {
    let mut client = RECT::default();
    if unsafe { GetClientRect(window, &mut client) }.is_err() {
        return;
    }
    unsafe {
        if high_contrast() {
            FillRect(dc, &client, GetSysColorBrush(COLOR_HIGHLIGHT));
        } else {
            let brush = CreateSolidBrush(COLOUR);
            FillRect(dc, &client, brush);
            let _ = DeleteObject(brush.into());
        }
    }
}

pub struct Border {
    windows: Vec<OwnedWindow>,
}

impl Border {
    pub fn create(instance: HINSTANCE, area: Rect, monitor: Rect, scale: f32) -> Result<Self> {
        let thickness = ((THICKNESS * scale).round() as i32).max(1);
        let mut windows = Vec::with_capacity(4);
        for side in recording::border(area, monitor, thickness) {
            let window = OwnedWindow(unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    BORDER_CLASS,
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
