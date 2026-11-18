//! The input method on the overlay: its composition read as it changes, for the overlay to draw
//! in the text rather than in the input method's own window, its candidate list placed under it,
//! and the input method turned on only while a text is typed.

use std::cell::Cell;

use bediz_core::markup::composition::Composition;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::Input::Ime::{
    CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM, CPS_CANCEL, GCS_COMPATTR,
    GCS_COMPCLAUSE, GCS_COMPREADATTR, GCS_COMPREADCLAUSE, GCS_COMPREADSTR, GCS_COMPSTR,
    GCS_CURSORPOS, GCS_DELTASTART, GCS_RESULTCLAUSE, GCS_RESULTREADCLAUSE, GCS_RESULTREADSTR,
    GCS_RESULTSTR, HIMC, IACE_DEFAULT, IME_COMPOSITION_STRING, ISC_SHOWUICOMPOSITIONWINDOW,
    ImmAssociateContext, ImmAssociateContextEx, ImmGetCompositionStringW, ImmGetContext,
    ImmNotifyIME, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow,
    NI_COMPOSITIONSTR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT,
};

const COMPOSITION_CHANGES: [IME_COMPOSITION_STRING; 4] =
    [GCS_COMPSTR, GCS_COMPATTR, GCS_COMPCLAUSE, GCS_CURSORPOS];

const EVERY_STRING: [IME_COMPOSITION_STRING; 12] = [
    GCS_COMPREADSTR,
    GCS_COMPREADATTR,
    GCS_COMPREADCLAUSE,
    GCS_COMPSTR,
    GCS_COMPATTR,
    GCS_COMPCLAUSE,
    GCS_CURSORPOS,
    GCS_DELTASTART,
    GCS_RESULTREADSTR,
    GCS_RESULTREADCLAUSE,
    GCS_RESULTSTR,
    GCS_RESULTCLAUSE,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Cancelled,
    Strings { result: bool, composition: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    Composing(Option<Composition>),
    Finished(String),
}

thread_local! {
    static HEARD: Cell<Vec<Update>> = const { Cell::new(Vec::new()) };
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
}

pub fn answer(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_IME_SETCONTEXT => unsafe {
            DefWindowProcW(window, message, wparam, without_composition_window(lparam))
        },
        // Passed on, the result would come again as one WM_CHAR per character.
        WM_IME_COMPOSITION => {
            let updates = read(window, lparam);
            heard(window, updates);
            LRESULT(0)
        }
        WM_IME_ENDCOMPOSITION => {
            heard(window, vec![Update::Composing(None)]);
            LRESULT(0)
        }
        _ => LRESULT(0),
    }
}

pub fn take() -> Option<(HWND, Vec<Update>)> {
    let updates = HEARD.with(Cell::take);
    let window = WINDOW.with(Cell::get)?;
    (!updates.is_empty()).then_some((window, updates))
}

pub fn place(window: HWND, area: RECT) {
    let context = unsafe { ImmGetContext(window) };
    if context.is_invalid() {
        return;
    }
    let at = POINT {
        x: area.left,
        y: area.top,
    };
    let composition = COMPOSITIONFORM {
        dwStyle: CFS_POINT,
        ptCurrentPos: at,
        ..Default::default()
    };
    let candidates = CANDIDATEFORM {
        dwIndex: 0,
        dwStyle: CFS_EXCLUDE,
        ptCurrentPos: at,
        rcArea: area,
    };
    unsafe {
        let _ = ImmSetCompositionWindow(context, &composition);
        let _ = ImmSetCandidateWindow(context, &candidates);
        let _ = ImmReleaseContext(window, context);
    }
}

pub fn cancel() {
    if let Some(window) = WINDOW.with(Cell::get) {
        cancel_in(window);
    }
}

// Outside a text the keys are the tools' own, which an input method left on would take.
pub fn allow(window: HWND, typing: bool) {
    if typing {
        let _ = unsafe { ImmAssociateContextEx(window, HIMC::default(), IACE_DEFAULT) };
    } else {
        cancel_in(window);
        let _ = unsafe { ImmAssociateContext(window, HIMC::default()) };
    }
}

fn cancel_in(window: HWND) {
    let context = unsafe { ImmGetContext(window) };
    if context.is_invalid() {
        return;
    }
    unsafe {
        let _ = ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
        let _ = ImmReleaseContext(window, context);
    }
}

fn without_composition_window(lparam: LPARAM) -> LPARAM {
    LPARAM(lparam.0 & !(ISC_SHOWUICOMPOSITIONWINDOW as isize))
}

fn heard(window: HWND, updates: Vec<Update>) {
    WINDOW.with(|heard_in| heard_in.set(Some(window)));
    HEARD.with(|heard| {
        let mut all = heard.take();
        all.extend(updates);
        heard.set(all);
    });
}

fn change_in(lparam: LPARAM) -> Change {
    let flags = lparam.0 as u32;
    let has = |kinds: &[IME_COMPOSITION_STRING]| kinds.iter().any(|kind| flags & kind.0 != 0);
    // Only a message naming no string at all cancels; the others update parts left unread here.
    if !has(&EVERY_STRING) {
        return Change::Cancelled;
    }
    Change::Strings {
        result: has(&[GCS_RESULTSTR]),
        composition: has(&COMPOSITION_CHANGES),
    }
}

fn read(window: HWND, lparam: LPARAM) -> Vec<Update> {
    let (result, composition) = match change_in(lparam) {
        Change::Cancelled => return vec![Update::Composing(None)],
        Change::Strings {
            result: false,
            composition: false,
        } => return Vec::new(),
        Change::Strings {
            result,
            composition,
        } => (result, composition),
    };
    let context = unsafe { ImmGetContext(window) };
    if context.is_invalid() {
        return Vec::new();
    }
    let mut updates = Vec::new();
    if result {
        let result: Vec<u16> = read_items(context, GCS_RESULTSTR);
        updates.push(Update::Finished(String::from_utf16_lossy(&result)));
    }
    if composition {
        let units: Vec<u16> = read_items(context, GCS_COMPSTR);
        let attributes: Vec<u8> = read_items(context, GCS_COMPATTR);
        let clause_starts: Vec<u32> = read_items(context, GCS_COMPCLAUSE);
        let caret = unsafe { ImmGetCompositionStringW(context, GCS_CURSORPOS, None, 0) };
        let caret = usize::try_from(caret).unwrap_or(units.len());
        let composition =
            Composition::from_input_method(&units, &attributes, &clause_starts, caret);
        updates.push(Update::Composing(composition));
    }
    let _ = unsafe { ImmReleaseContext(window, context) };
    updates
}

fn read_items<T: Copy + Default>(context: HIMC, kind: IME_COMPOSITION_STRING) -> Vec<T> {
    let size = unsafe { ImmGetCompositionStringW(context, kind, None, 0) };
    let count = usize::try_from(size).unwrap_or(0) / size_of::<T>();
    if count == 0 {
        return Vec::new();
    }
    let mut items = vec![T::default(); count];
    let Ok(length) = u32::try_from(count * size_of::<T>()) else {
        return Vec::new();
    };
    let copied =
        unsafe { ImmGetCompositionStringW(context, kind, Some(items.as_mut_ptr().cast()), length) };
    items.truncate(usize::try_from(copied).unwrap_or(0) / size_of::<T>());
    items
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::Input::Ime::{CS_INSERTCHAR, CS_NOMOVECARET};

    use super::*;

    fn flags(kinds: &[IME_COMPOSITION_STRING]) -> LPARAM {
        LPARAM(kinds.iter().fold(0, |flags, kind| flags | kind.0) as isize)
    }

    #[test]
    fn only_a_message_naming_no_string_cancels_the_composition() {
        assert_eq!(change_in(LPARAM(0)), Change::Cancelled);
        let caret_only = LPARAM((CS_INSERTCHAR | CS_NOMOVECARET) as isize);
        assert_eq!(change_in(caret_only), Change::Cancelled);
        let untouched = Change::Strings {
            result: false,
            composition: false,
        };
        for reading in [
            GCS_COMPREADSTR,
            GCS_COMPREADATTR,
            GCS_COMPREADCLAUSE,
            GCS_DELTASTART,
        ] {
            assert_eq!(change_in(flags(&[reading])), untouched);
        }
        assert_eq!(
            change_in(flags(&[GCS_COMPSTR, GCS_COMPATTR, GCS_DELTASTART])),
            Change::Strings {
                result: false,
                composition: true,
            }
        );
        assert_eq!(
            change_in(flags(&[GCS_RESULTSTR, GCS_RESULTCLAUSE])),
            Change::Strings {
                result: true,
                composition: false,
            }
        );
    }

    #[test]
    fn a_reading_update_keeps_the_composition_and_a_cancel_or_the_end_drops_it() {
        let window = HWND(std::ptr::dangling_mut());
        answer(
            window,
            WM_IME_COMPOSITION,
            WPARAM(0),
            flags(&[GCS_COMPREADSTR]),
        );
        assert_eq!(take(), None);
        answer(window, WM_IME_COMPOSITION, WPARAM(0), LPARAM(0));
        answer(window, WM_IME_ENDCOMPOSITION, WPARAM(0), LPARAM(0));
        assert_eq!(
            take(),
            Some((
                window,
                vec![Update::Composing(None), Update::Composing(None)]
            ))
        );
    }

    #[test]
    fn the_overlay_draws_the_composition_and_leaves_the_rest_to_the_input_method() {
        let everything = LPARAM(0xC000_000F_u32 as isize);
        assert_eq!(without_composition_window(everything), LPARAM(0x4000_000F));
    }

    #[test]
    fn what_the_input_method_said_is_taken_in_order_and_only_once() {
        let window = HWND(std::ptr::dangling_mut());
        heard(window, vec![Update::Finished("日本".to_owned())]);
        heard(window, vec![Update::Composing(None)]);
        assert_eq!(
            take(),
            Some((
                window,
                vec![Update::Finished("日本".to_owned()), Update::Composing(None)]
            ))
        );
        assert_eq!(take(), None);
    }
}
