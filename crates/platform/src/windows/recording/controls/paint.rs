use std::ffi::c_void;

use bediz_core::geometry::Rect;
use windows::Win32::Foundation::{COLORREF, HWND, LRESULT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_BTNTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW,
    COLOR_WINDOWTEXT, CreateSolidBrush, DRAW_TEXT_FORMAT, DT_CENTER, DT_LEFT, DT_RTLREADING,
    DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, Ellipse, FillRect, GetStockObject,
    GetSysColor, GetSysColorBrush, GetTextExtentPoint32W, HBRUSH, HDC, HFONT, NULL_PEN, RoundRect,
    SYS_COLOR_INDEX, SelectObject, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_SELECTED};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GCLP_HBRBACKGROUND, GWL_EXSTYLE, GetClassLongPtrW, GetClientRect, GetWindowLongW,
    GetWindowTextLengthW, GetWindowTextW, SendMessageW, WM_GETFONT, WS_EX_LAYOUTRTL,
};

use super::fonts::is_icon_face;
use super::layout::{MARK, MARK_GAP, grip_at};
use super::{Button, Face, pixels, scale};
use crate::windows::contrast::high_contrast;

pub const GROUND: COLORREF = COLORREF(0x0020_2020);

const RADIUS: f32 = 6.0;
const DOT: f32 = 3.0;
const DOT_GAP: f32 = 3.0;

struct Palette {
    ground: u32,
    text: u32,
    face: u32,
    pressed: u32,
    ink: u32,
    pressed_ink: u32,
    mark: u32,
    grip: u32,
}

const DARK: Palette = Palette {
    ground: GROUND.0,
    text: 0x00FF_FFFF,
    face: 0x0038_3838,
    pressed: 0x0050_5050,
    ink: 0x00FF_FFFF,
    pressed_ink: 0x00FF_FFFF,
    mark: 0x0023_11E8,
    grip: 0x0090_9090,
};

pub fn erase(window: HWND, dc: HDC, grip: bool) {
    let mut client = RECT::default();
    if unsafe { GetClientRect(window, &mut client) }.is_err() {
        return;
    }
    unsafe { FillRect(dc, &client, ground_brush(window)) };
    if grip {
        let scale = scale(unsafe { GetDpiForWindow(window) });
        draw_grip(
            dc,
            grip_at(scale, client.bottom - client.top),
            scale,
            palette().grip,
        );
    }
}

pub fn colour_static(window: HWND, dc: HDC) -> LRESULT {
    let palette = palette();
    unsafe {
        SetTextColor(dc, COLORREF(palette.text));
        SetBkColor(dc, COLORREF(palette.ground));
    }
    LRESULT(ground_brush(window).0 as isize)
}

pub fn draw_button(item: &DRAWITEMSTRUCT) {
    let palette = palette();
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let (face, ink) = if pressed {
        (palette.pressed, palette.pressed_ink)
    } else {
        (palette.face, palette.ink)
    };
    let scale = scale(unsafe { GetDpiForWindow(item.hwndItem) });
    let dc = item.hDC;
    let whole = item.rcItem;
    fill(dc, &whole, palette.ground);
    let radius = pixels(RADIUS, scale);
    filled(dc, face, || unsafe {
        let _ = RoundRect(
            dc,
            whole.left,
            whole.top,
            whole.right + 1,
            whole.bottom + 1,
            radius,
            radius,
        );
    });
    let font = unsafe { SendMessageW(item.hwndItem, WM_GETFONT, None, None) };
    let previous_font = unsafe { SelectObject(dc, HFONT(font.0 as *mut c_void).into()) };
    let reading = if right_to_left(item.hwndItem) {
        DT_RTLREADING
    } else {
        DRAW_TEXT_FORMAT(0)
    };
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(ink));
    }
    let side = pixels(MARK, scale);
    let middle = (whole.top + whole.bottom) / 2 - side / 2;
    let button_face = u16::try_from(item.CtlID)
        .ok()
        .and_then(Button::from_id)
        .map(Button::face);
    match button_face {
        Some(Face::Labelled) => {
            let mut text = window_text(item.hwndItem);
            let mut size = SIZE::default();
            let _ = unsafe { GetTextExtentPoint32W(dc, &text, &mut size) };
            let mark_gap = pixels(MARK_GAP, scale);
            let width = whole.right - whole.left;
            let left = whole.left + (width - side - mark_gap - size.cx).max(0) / 2;
            draw_dot(dc, &square(left, middle, side), palette.mark);
            let mut text_area = RECT {
                left: left + side + mark_gap,
                ..whole
            };
            unsafe {
                DrawTextW(
                    dc,
                    &mut text,
                    &mut text_area,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | reading,
                )
            };
        }
        Some(Face::Square) => {
            let left = (whole.left + whole.right) / 2 - side / 2;
            fill(dc, &square(left, middle, side), palette.mark);
        }
        Some(Face::Glyph(glyph)) if is_icon_face(dc) => {
            centred(dc, &mut [glyph], whole, reading);
        }
        Some(Face::Glyph(_)) | None => {
            centred(dc, &mut window_text(item.hwndItem), whole, reading);
        }
    }
    unsafe { SelectObject(dc, previous_font) };
}

fn centred(dc: HDC, text: &mut [u16], whole: RECT, reading: DRAW_TEXT_FORMAT) {
    let mut area = whole;
    unsafe {
        DrawTextW(
            dc,
            text,
            &mut area,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | reading,
        )
    };
}

fn square(left: i32, top: i32, side: i32) -> RECT {
    RECT {
        left,
        top,
        right: left + side,
        bottom: top + side,
    }
}

fn draw_dot(dc: HDC, rect: &RECT, colour: u32) {
    filled(dc, colour, || unsafe {
        let _ = Ellipse(dc, rect.left, rect.top, rect.right + 1, rect.bottom + 1);
    });
}

fn draw_grip(dc: HDC, area: Rect, scale: f32, colour: u32) {
    let dot = pixels(DOT, scale).max(2);
    let step = dot + pixels(DOT_GAP, scale).max(1);
    let half = |length: u32, used: i32| (i32::try_from(length).unwrap_or(0) - used) / 2;
    let left = area.left + half(area.width(), dot + step);
    let top = area.top + half(area.height(), dot + 2 * step);
    for row in 0..3 {
        for column in 0..2 {
            draw_dot(
                dc,
                &square(left + column * step, top + row * step, dot),
                colour,
            );
        }
    }
}

fn filled(dc: HDC, colour: u32, draw: impl FnOnce()) {
    unsafe {
        let brush = CreateSolidBrush(COLORREF(colour));
        let previous_brush = SelectObject(dc, brush.into());
        let previous_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        draw();
        SelectObject(dc, previous_pen);
        SelectObject(dc, previous_brush);
        let _ = DeleteObject(brush.into());
    }
}

fn palette() -> Palette {
    if !high_contrast() {
        return DARK;
    }
    let system = |index: SYS_COLOR_INDEX| unsafe { GetSysColor(index) };
    Palette {
        ground: system(COLOR_WINDOW),
        text: system(COLOR_WINDOWTEXT),
        face: system(COLOR_BTNFACE),
        pressed: system(COLOR_HIGHLIGHT),
        ink: system(COLOR_BTNTEXT),
        pressed_ink: system(COLOR_HIGHLIGHTTEXT),
        mark: system(COLOR_BTNTEXT),
        grip: system(COLOR_WINDOWTEXT),
    }
}

fn ground_brush(window: HWND) -> HBRUSH {
    if high_contrast() {
        return unsafe { GetSysColorBrush(COLOR_WINDOW) };
    }
    HBRUSH(unsafe { GetClassLongPtrW(window, GCLP_HBRBACKGROUND) } as *mut c_void)
}

fn fill(dc: HDC, rect: &RECT, colour: u32) {
    unsafe {
        let brush = CreateSolidBrush(COLORREF(colour));
        FillRect(dc, rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

fn right_to_left(window: HWND) -> bool {
    let extended = unsafe { GetWindowLongW(window, GWL_EXSTYLE) } as u32;
    extended & WS_EX_LAYOUTRTL.0 != 0
}

fn window_text(window: HWND) -> Vec<u16> {
    let length = unsafe { GetWindowTextLengthW(window) };
    let mut text = vec![0u16; usize::try_from(length).unwrap_or(0) + 1];
    let copied = unsafe { GetWindowTextW(window, &mut text) };
    text.truncate(usize::try_from(copied).unwrap_or(0));
    text
}
