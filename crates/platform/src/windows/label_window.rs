use std::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Factory,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_READING_DIRECTION_RIGHT_TO_LEFT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS,
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetMonitorInfoW,
    HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, SelectObject,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_TOPMOST, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SetWindowDisplayAffinity, SetWindowPos, ShowWindow, ULW_ALPHA,
    USER_DEFAULT_SCREEN_DPI, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Error, HSTRING, Result, w};

use super::contrast::Colours;
use super::window_class::LABEL_CLASS;
use crate::event::Label;

const FONT_SIZE: f32 = 14.0;
const PADDING_X: f32 = 14.0;
const PADDING_Y: f32 = 9.0;
const RADIUS: f32 = 6.0;
const MAX_TEXT_WIDTH: f32 = 420.0;
const MARGIN: f32 = 8.0;
const TRANSPARENT: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};
const BACKGROUND: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.12,
    g: 0.12,
    b: 0.12,
    a: 0.94,
};
const WARNING: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.6,
    g: 0.11,
    b: 0.11,
    a: 0.96,
};
const TEXT: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

struct OwnedWindow(HWND);

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

struct MemoryBitmap {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl MemoryBitmap {
    fn new(width: i32, height: i32) -> Result<Self> {
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return Err(Error::from_thread());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = match unsafe {
            CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        } {
            Ok(bitmap) => bitmap,
            Err(error) => {
                let _ = unsafe { DeleteDC(dc) };
                return Err(error);
            }
        };
        let previous = unsafe { SelectObject(dc, bitmap.into()) };
        Ok(Self {
            dc,
            bitmap,
            previous,
        })
    }
}

impl Drop for MemoryBitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct LabelWindow {
    window: OwnedWindow,
    d2d: ID2D1Factory,
    text: IDWriteFactory,
}

impl LabelWindow {
    pub fn create(instance: HINSTANCE) -> Result<Self> {
        let window = OwnedWindow(unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                LABEL_CLASS,
                w!("Bediz"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
        }?);
        unsafe { SetWindowDisplayAffinity(window.0, WDA_EXCLUDEFROMCAPTURE) }?;
        Ok(Self {
            window,
            d2d: unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?,
            text: unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?,
        })
    }

    pub fn show(&self, label: &Label, colours: Option<&Colours>) -> Result<()> {
        let center = POINT {
            x: label.near.left + i32::try_from(label.near.width() / 2).unwrap_or(0),
            y: label.near.top + i32::try_from(label.near.height() / 2).unwrap_or(0),
        };
        let monitor = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            info.rcWork
        } else {
            RECT {
                left: center.x,
                top: center.y,
                right: center.x,
                bottom: center.y,
            }
        };
        let (mut dpi, mut unused) = (0, 0);
        let scale =
            match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) } {
                Ok(()) if dpi > 0 => dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32,
                _ => 1.0,
            };

        let format = unsafe {
            self.text.CreateTextFormat(
                w!("Segoe UI"),
                None::<&IDWriteFontCollection>,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                FONT_SIZE * scale,
                &HSTRING::from(label.language.as_str()),
            )
        }?;
        unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER) }?;
        if label.right_to_left {
            unsafe { format.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT) }?;
        }
        let wide: Vec<u16> = label.lines.join("\n").encode_utf16().collect();
        let layout = unsafe {
            self.text
                .CreateTextLayout(&wide, &format, MAX_TEXT_WIDTH * scale, f32::MAX)
        }?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics) }?;
        let (padding_x, padding_y) = (PADDING_X * scale, PADDING_Y * scale);
        let text_width = metrics.width.ceil();
        let width = (text_width + 2.0 * padding_x).ceil() as i32;
        let height = (metrics.height + 2.0 * padding_y).ceil() as i32;
        let margin = (MARGIN * scale).round() as i32;
        let x = (center.x - width / 2)
            .min(work.right - margin - width)
            .max(work.left + margin);
        let y = (center.y - height / 2)
            .min(work.bottom - margin - height)
            .max(work.top + margin);

        let canvas = MemoryBitmap::new(width, height)?;
        let target = unsafe {
            self.d2d
                .CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: USER_DEFAULT_SCREEN_DPI as f32,
                    dpiY: USER_DEFAULT_SCREEN_DPI as f32,
                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                    minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                })
        }?;
        let bounds = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        unsafe { target.BindDC(canvas.dc, &bounds) }?;
        let (background, text) = match colours {
            Some(colours) => (colours.ground, colours.ink),
            None if label.warning => (WARNING, TEXT),
            None => (BACKGROUND, TEXT),
        };
        let fill = unsafe { target.CreateSolidColorBrush(&background, None) }?;
        let ink = unsafe { target.CreateSolidColorBrush(&text, None) }?;
        let radius = RADIUS * scale;
        let whole = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: width as f32,
            bottom: height as f32,
        };
        let text_area = D2D_RECT_F {
            left: (width as f32 - text_width) / 2.0,
            top: padding_y,
            right: (width as f32 + text_width) / 2.0,
            bottom: height as f32 - padding_y,
        };
        unsafe {
            target.BeginDraw();
            // ClearType needs an opaque target; this one has transparent corners.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            target.Clear(Some(&TRANSPARENT));
            target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: whole,
                    radiusX: radius,
                    radiusY: radius,
                },
                &fill,
            );
            target.DrawText(
                &wide,
                &format,
                &text_area,
                &ink,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            target.EndDraw(None, None)?;
        }

        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: u8::MAX,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                self.window.0,
                None,
                Some(&POINT { x, y }),
                Some(&SIZE {
                    cx: width,
                    cy: height,
                }),
                Some(canvas.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )?;
            SetWindowPos(
                self.window.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
    }

    pub fn hide(&self) {
        let _ = unsafe { ShowWindow(self.window.0, SW_HIDE) };
    }
}
