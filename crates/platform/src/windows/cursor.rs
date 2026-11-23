//! The cursor over the overlay: one of Windows' own, or a pen's or highlighter's ring made at its
//! physical size when the cursor can carry it, which is destroyed only once another is in use.

use std::cell::Cell;

use bediz_core::geometry::{Point, Rect};
use bediz_core::markup::ring::{self, Ink, Pixels};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_CREATION_SCALING_NONE, CreateCursor, DestroyCursor, GetCursor, GetSystemMetrics,
    HCURSOR, IDC_ARROW, IDC_CROSS, LoadCursorW, SM_CXCURSOR, SM_REMOTESESSION, SPI_GETMOUSETRAILS,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCursor, SystemParametersInfoW,
};
use windows::core::{PCWSTR, Result, s, w};

// Larger cursors were seen leaving the hardware plane, where they lag as the overlay does.
const LARGEST: usize = 127;

type SetScaling = unsafe extern "system" fn(u32) -> u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    across: f32,
    dpi: u32,
    // Windows was seen cutting a cursor off at the edge of the monitor under its hot spot.
    spills: bool,
}

impl Ring {
    pub fn at(across: f32, scale: f32, point: Point, monitors: &[Rect]) -> Self {
        Self {
            across,
            dpi: (scale * 96.0).round() as u32,
            spills: ring::spills(point, across, monitors),
        }
    }

    pub fn moved(self, point: Point, monitors: &[Rect]) -> Self {
        Self {
            spills: ring::spills(point, self.across, monitors),
            ..self
        }
    }

    pub fn across(self) -> u32 {
        self.across.round() as u32
    }
}

struct Made {
    cursor: HCURSOR,
    ring: Ring,
    carried: bool,
}

impl Drop for Made {
    fn drop(&mut self) {
        let _ = unsafe { DestroyCursor(self.cursor) };
    }
}

thread_local! {
    static SHOWN: Cell<Option<HCURSOR>> = const { Cell::new(None) };
    static MADE: Cell<Option<Made>> = const { Cell::new(None) };
}

pub fn shown() -> Option<HCURSOR> {
    SHOWN.with(Cell::get)
}

pub fn is_ours() -> bool {
    shown().is_some_and(|cursor| unsafe { GetCursor() } == cursor)
}

pub fn ring_shown() -> Option<(Ring, bool)> {
    let made = MADE.take();
    let shown = made
        .as_ref()
        .filter(|made| unsafe { GetCursor() } == made.cursor)
        .map(|made| (made.ring, made.carried));
    MADE.set(made);
    shown
}

// A trail repeats the cursor, and a remote client may drop one larger than its own limit.
fn carries(ring: Ring) -> bool {
    !ring.spills && ring::fits(ring.across, LARGEST) && !remote() && !trailing()
}

pub fn show(shape: PCWSTR) {
    if let Ok(cursor) = unsafe { LoadCursorW(None, shape) } {
        set(cursor);
        drop(MADE.take());
    }
}

pub fn show_ring(ring: Ring) {
    match MADE.take() {
        Some(made) if made.ring == ring => {
            set(made.cursor);
            MADE.set(Some(made));
        }
        kept => match make(ring) {
            Ok(made) => {
                set(made.cursor);
                MADE.set(Some(made));
                drop(kept);
            }
            Err(_) => {
                MADE.set(kept);
                show(IDC_CROSS);
            }
        },
    }
}

pub fn let_go() {
    let Some(made) = MADE.take() else {
        return;
    };
    if unsafe { GetCursor() } == made.cursor {
        match unsafe { LoadCursorW(None, IDC_ARROW) } {
            Ok(arrow) => set(arrow),
            Err(_) => {
                MADE.set(Some(made));
                return;
            }
        }
    }
    if shown() == Some(made.cursor) {
        SHOWN.set(None);
    }
    drop(made);
}

fn set(cursor: HCURSOR) {
    SHOWN.set(Some(cursor));
    unsafe { SetCursor(Some(cursor)) };
}

fn make(ring: Ring) -> Result<Made> {
    let nominal = unsafe { GetSystemMetricsForDpi(SM_CXCURSOR, ring.dpi) };
    let arm = u32::try_from(nominal).unwrap_or(0) * 5 / 16;
    let carried = carries(ring);
    let pixels = ring::pixels(carried.then_some(ring.across), arm, LARGEST);
    let side = pixels.side() as i32;
    let centre = pixels.centre() as i32;
    let and = plane(&pixels, Ink::Clear);
    let xor = plane(&pixels, Ink::Light);
    let previous = set_scaling(CURSOR_CREATION_SCALING_NONE);
    let made = unsafe {
        CreateCursor(
            None,
            centre,
            centre,
            side,
            side,
            and.as_ptr().cast(),
            xor.as_ptr().cast(),
        )
    };
    if let Some(previous) = previous {
        set_scaling(previous);
    }
    made.map(|cursor| Made {
        cursor,
        ring,
        carried,
    })
}

// Rows of a monochrome bitmap start on a 16-bit boundary, their leftmost pixel in the highest bit.
fn plane(pixels: &Pixels, set: Ink) -> Vec<u8> {
    let stride = pixels.side().div_ceil(16) * 2;
    pixels
        .rows()
        .flat_map(|row| {
            (0..stride).map(move |byte| {
                (0..8).fold(0, |bits, bit| {
                    let on = row.get(byte * 8 + bit) == Some(&set);
                    bits | (u8::from(on) << (7 - bit))
                })
            })
        })
        .collect()
}

fn remote() -> bool {
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

fn trailing() -> bool {
    let mut trail = 0i32;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETMOUSETRAILS,
            0,
            Some(std::ptr::from_mut(&mut trail).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && trail > 1
}

// windows-rs has no binding for it, and a lookup keeps a missing export from stopping the start.
fn set_scaling(dpi: u32) -> Option<u32> {
    let module = unsafe { GetModuleHandleW(w!("user32.dll")) }.ok()?;
    let found = unsafe { GetProcAddress(module, s!("SetThreadCursorCreationScaling")) }?;
    let set =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, SetScaling>(found) };
    Some(unsafe { set(dpi) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_row_is_padded_to_two_bytes_with_its_first_pixel_in_the_top_bit() {
        let pixels = ring::pixels(None, 4, LARGEST);
        assert_eq!(pixels.side(), 11);
        let clear = plane(&pixels, Ink::Clear);
        let light = plane(&pixels, Ink::Light);
        assert_eq!(clear.len(), 11 * 2);
        assert_eq!(light.len(), 11 * 2);
        let centre_row = 5 * 2;
        assert_eq!(
            &clear[centre_row..centre_row + 2],
            &[0b0000_1110, 0b0000_0000]
        );
        assert_eq!(
            &light[centre_row..centre_row + 2],
            &[0b1000_0000, 0b0010_0000]
        );
        assert_eq!(&clear[0..2], &[0b1111_0001, 0b1110_0000]);
        assert_eq!(&light[0..2], &[0b0000_1110, 0b0000_0000]);
    }

    const MONITORS: [Rect; 2] = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 3840, 1080)];

    #[test]
    fn a_cursor_is_made_again_only_when_its_ring_or_monitor_changes() {
        let middle = Point::new(900, 500);
        let pen = Ring::at(3.0, 1.0, middle, &MONITORS);
        assert_eq!(pen, Ring::at(3.0, 1.0, middle, &MONITORS));
        assert_eq!(pen, Ring::at(3.0, 1.0, Point::new(1000, 600), &MONITORS));
        assert_ne!(pen, Ring::at(4.0, 1.0, middle, &MONITORS));
        assert_ne!(pen, Ring::at(3.0, 1.5, middle, &MONITORS));
        assert_eq!(Ring::at(3.0, 1.5, middle, &MONITORS).dpi, 144);
    }

    #[test]
    fn the_cursor_leaves_a_ring_reaching_another_monitor_to_the_overlay() {
        let highlighter = Ring::at(120.0, 1.0, Point::new(900, 500), &MONITORS);
        let at_edge = highlighter.moved(Point::new(1900, 500), &MONITORS);
        assert_ne!(highlighter, at_edge);
        assert!(!carries(at_edge));
        assert_eq!(at_edge.moved(Point::new(900, 500), &MONITORS), highlighter);
        assert_eq!(
            Ring::at(120.0, 1.0, Point::new(1900, 500), &MONITORS),
            at_edge
        );
    }
}
