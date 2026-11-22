//! The monitor under a point, a rectangle or a window: its bounds, its work area and its scale.

use bediz_core::geometry::{Point, Rect};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    MonitorFromRect, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::USER_DEFAULT_SCREEN_DPI;

#[derive(Clone, Copy)]
pub struct Monitor(HMONITOR);

impl Monitor {
    pub fn at(point: Point) -> Self {
        let point = POINT {
            x: point.x,
            y: point.y,
        };
        Self(unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) })
    }

    pub fn near(rect: Rect) -> Self {
        let bounds = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        Self(unsafe { MonitorFromRect(&bounds, MONITOR_DEFAULTTONEAREST) })
    }

    pub fn of(window: HWND) -> Self {
        Self(unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) })
    }

    pub fn bounds(self) -> Option<Rect> {
        self.info().map(|info| rect_of(info.rcMonitor))
    }

    pub fn work(self) -> Option<Rect> {
        self.info().map(|info| rect_of(info.rcWork))
    }

    pub fn dpi(self) -> u32 {
        let (mut dpi, mut unused) = (0, 0);
        match unsafe { GetDpiForMonitor(self.0, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) } {
            Ok(()) if dpi > 0 => dpi,
            _ => USER_DEFAULT_SCREEN_DPI,
        }
    }

    pub fn scale(self) -> f32 {
        scale_of(self.dpi())
    }

    fn info(self) -> Option<MONITORINFO> {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetMonitorInfoW(self.0, &mut info) }
            .as_bool()
            .then_some(info)
    }
}

pub fn scale_of(dpi: u32) -> f32 {
    dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32
}

fn rect_of(bounds: RECT) -> Rect {
    Rect::new(bounds.left, bounds.top, bounds.right, bounds.bottom)
}
