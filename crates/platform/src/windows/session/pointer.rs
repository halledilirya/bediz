//! The selection's side of the mouse: a press that grabs the selection, starts a new one or waits
//! beside it for a drag, a click that selects the window under it, letting go, the pointer's
//! shape over the selection, and a pen's ring and width the overlay draws beside it.

use bediz_core::geometry::{DragDistance, Point, Rect};
use bediz_core::markup::{Tool, ring};
use bediz_core::selection::{Grip, Handle, Selection};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, IDC_ARROW, IDC_CROSS, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW,
    IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, SM_CXDRAG, SM_CYDRAG, WindowFromPoint,
};
use windows::core::PCWSTR;

use super::Session;
use super::keys::modifiers;
use crate::windows::cursor;
use crate::windows::overlay::Brush;
use crate::windows::scene::Scene;
use crate::windows::window_list::WindowList;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Look {
    Shared(PCWSTR),
    Ring(cursor::Ring),
}

#[derive(Default)]
pub(super) struct Hovering {
    pub(super) width_at: Option<Point>,
    away: bool,
}

pub(super) struct Click {
    at: Point,
    pub(super) window: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    Grab(Grip),
    Beside,
    Start,
}

impl Session {
    pub(super) fn inside_selection(&self, scene: &Scene, point: Point) -> bool {
        self.selection.is_some_and(|selection| {
            !selection.is_dragging()
                && self.grip(&selection, point, scene.reach_at(point)) == Some(Grip::Inside)
        })
    }

    pub(super) fn press(&mut self, scene: &Scene, windows: Option<&WindowList>, point: Point) {
        if !self.purpose.reshapes() {
            return;
        }
        match press_on(self.selection.as_ref(), point, scene.reach_at(point)) {
            Press::Grab(grip) => {
                if let Some(selection) = &mut self.selection {
                    selection.grab(grip, point);
                }
                cursor::show(cursor_for(Some(grip)));
            }
            Press::Beside => self.beside = Some(point),
            Press::Start => {
                self.selection = Selection::start(scene.monitor_rects(), point);
                self.click = windows
                    .and_then(|windows| windows.window_at(point))
                    .map(|window| Click { at: point, window });
                cursor::show(IDC_CROSS);
            }
        }
    }

    pub(super) fn hover(&mut self, scene: &Scene, point: Point) -> bool {
        if self.choosing_words() {
            return self.drag_words(point);
        }
        if self.marks_busy() {
            self.drag_mark(point);
            if let Some((ring, _)) = cursor::ring_shown() {
                cursor::show_ring(ring.moved(point, scene.monitor_rects()));
            }
            return true;
        }
        if let Some(at) = self.beside {
            if !beyond_drag_threshold(at, point) {
                return false;
            }
            self.beside = None;
            self.selection = Selection::start(scene.monitor_rects(), at);
            cursor::show(IDC_CROSS);
        }
        if let Some(click) = &self.click {
            if !beyond_drag_threshold(click.at, point) {
                return true;
            }
            self.click = None;
        }
        match &mut self.selection {
            Some(selection) if selection.is_dragging() => {
                selection.drag(point, modifiers());
                true
            }
            Some(selection) => {
                let selection = *selection;
                show(self.look_at(scene, &selection, point));
                false
            }
            None => true,
        }
    }

    // A wheel, a key or the runtime's change over a still pointer sends no WM_SETCURSOR.
    pub fn point_again(&self, scene: &Scene) {
        let (Some(point), Some(selection)) = (self.pointer, self.selection) else {
            return;
        };
        if selection.is_dragging()
            || self.marks_busy()
            || self.choosing_words()
            || !cursor::is_ours()
        {
            return;
        }
        show(self.look_at(scene, &selection, point));
    }

    pub(super) fn show_width(&mut self) {
        self.hovering.width_at = self.pointer;
    }

    pub(super) fn entered(&mut self, window: HWND, point: Point) {
        self.hovering.away = false;
        if self
            .hovering
            .width_at
            .is_some_and(|at| beyond_drag_threshold(at, point))
        {
            self.hovering.width_at = None;
        }
        // A leave ends tracking even where the runtime misses it, so each move asks again.
        let mut leave = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: window,
            dwHoverTime: 0,
        };
        let _ = unsafe { TrackMouseEvent(&mut leave) };
    }

    // Moving onto another monitor's overlay leaves the first one too, and the ring stays.
    pub(super) fn left(&mut self, scene: &Scene) -> bool {
        let mut at = POINT::default();
        let on_overlay =
            unsafe { GetCursorPos(&mut at) }.is_ok() && scene.owns(unsafe { WindowFromPoint(at) });
        self.hovering.away = !on_overlay;
        self.hovering.away && self.views.iter().any(|view| view.brush.is_some())
    }

    pub(super) fn follows_pointer(&self) -> bool {
        self.brush().is_some() || self.views.iter().any(|view| view.brush.is_some())
    }

    pub(super) fn brush(&self) -> Option<Brush> {
        if self.hovering.away || self.is_dragging() || !self.purpose.is_still() {
            return None;
        }
        let at = self.pointer?;
        let shown =
            cursor::ring_shown().filter(|_| brushes(self.marking.tool_width(), self.shows_words()));
        let drawn = shown.is_some_and(|(_, carried)| !carried);
        let inside = cursor::is_ours() && self.selected().is_some_and(|area| area.contains(at));
        let width = self
            .hovering
            .width_at
            .filter(|_| inside)
            .and(self.marking.tool_width())
            .and_then(|(tool, width)| ring::stroke(tool, width))
            .map(|stroke| stroke.round() as u32);
        (drawn || width.is_some()).then(|| Brush {
            at,
            across: shown.map_or(0, |(ring, _)| ring.across()),
            ring: drawn,
            width,
        })
    }

    fn look_at(&self, scene: &Scene, selection: &Selection, point: Point) -> Look {
        let reach = scene.reach_at(point);
        let grip = self.grip(selection, point, reach);
        if self.shows_words() && grip == Some(Grip::Inside) {
            return Look::Shared(IDC_IBEAM);
        }
        self.mark_cursor(scene, point, grip, reach)
            .unwrap_or_else(|| {
                Look::Shared(if self.purpose.reshapes() {
                    cursor_for(grip)
                } else {
                    IDC_ARROW
                })
            })
    }

    pub(super) fn release(&mut self, scene: &Scene, window: HWND, point: Point) -> bool {
        if self.choosing_words() {
            return self.release_words(scene);
        }
        if self.marks_busy() {
            self.release_mark(scene, window, point);
            return true;
        }
        if self.beside.take().is_some() {
            return false;
        }
        if let Some(click) = self.click.take() {
            self.selection = Selection::restore(scene.monitor_rects(), click.window);
            return true;
        }
        match &mut self.selection {
            Some(selection) if selection.is_dragging() => {
                selection.finish(point, modifiers());
                true
            }
            _ => false,
        }
    }
}

// A stray click beside a selection must not lose it and its marks, so only a drag starts another.
fn press_on(selection: Option<&Selection>, at: Point, reach: i32) -> Press {
    let Some(selection) = selection else {
        return Press::Start;
    };
    match selection.grip_at(at, reach) {
        Some(grip) => Press::Grab(grip),
        None if selection.rect().is_some() => Press::Beside,
        None => Press::Start,
    }
}

fn brushes(tool_width: Option<(Tool, f32)>, words_shown: bool) -> bool {
    !words_shown
        && tool_width
            .and_then(|(tool, width)| ring::footprint(tool, width))
            .is_some()
}

fn show(look: Look) {
    match look {
        Look::Shared(shape) => cursor::show(shape),
        Look::Ring(ring) => cursor::show_ring(ring),
    }
}

pub(super) fn beyond_drag_threshold(from: Point, to: Point) -> bool {
    drag_distance().passed(from, to)
}

pub(super) fn drag_distance() -> DragDistance {
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
    DragDistance { width, height }
}

fn cursor_for(grip: Option<Grip>) -> PCWSTR {
    match grip {
        None => IDC_CROSS,
        Some(Grip::Inside) => IDC_SIZEALL,
        Some(Grip::Handle(Handle::TopLeft | Handle::BottomRight)) => IDC_SIZENWSE,
        Some(Grip::Handle(Handle::TopRight | Handle::BottomLeft)) => IDC_SIZENESW,
        Some(Grip::Handle(Handle::Left | Handle::Right)) => IDC_SIZEWE,
        Some(Grip::Handle(Handle::Top | Handle::Bottom)) => IDC_SIZENS,
    }
}

#[cfg(test)]
mod tests {
    use bediz_core::selection::Modifiers;

    use super::*;

    #[test]
    fn a_press_beside_a_selection_waits_and_one_on_it_grabs_it() {
        let monitors = [Rect::new(0, 0, 1920, 1080)];
        let made = Selection::restore(&monitors, Rect::new(100, 100, 400, 300));
        let pressed = |at: Point| press_on(made.as_ref(), at, 4);
        assert_eq!(pressed(Point::new(800, 600)), Press::Beside);
        assert_eq!(pressed(Point::new(250, 200)), Press::Grab(Grip::Inside));
        assert!(matches!(
            pressed(Point::new(100, 100)),
            Press::Grab(Grip::Handle(_))
        ));
        assert_eq!(press_on(None, Point::new(800, 600), 4), Press::Start);
        let Some(mut nothing) = Selection::start(&monitors, Point::new(10, 10)) else {
            panic!("a selection starts on a monitor");
        };
        nothing.finish(Point::new(10, 10), Modifiers::default());
        assert_eq!(nothing.rect(), None);
        assert_eq!(
            press_on(Some(&nothing), Point::new(800, 600), 4),
            Press::Start
        );
    }

    #[test]
    fn only_a_pen_or_highlighter_away_from_the_words_draws_its_ring() {
        assert!(brushes(Some((Tool::Pen, 3.0)), false));
        assert!(brushes(Some((Tool::Highlighter, 40.0)), false));
        assert!(!brushes(Some((Tool::Pen, 3.0)), true));
        assert!(!brushes(None, false));
        for tool in [Tool::Rectangle, Tool::Text, Tool::Step, Tool::Blur] {
            assert!(!brushes(Some((tool, 3.0)), false), "{tool:?}");
        }
    }
}
