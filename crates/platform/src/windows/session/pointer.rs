//! The selection's side of the mouse: a press that grabs the selection, starts a new one or waits
//! beside it for a drag, a click that selects the window under it, letting go, and the pointer's
//! shape over the selection.

use bediz_core::geometry::{Point, Rect};
use bediz_core::selection::{Grip, Handle, Selection};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, IDC_ARROW, IDC_CROSS, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
    IDC_SIZEWE, SM_CXDRAG, SM_CYDRAG,
};
use windows::core::PCWSTR;

use super::{Session, modifiers, show_cursor};
use crate::windows::scene::Scene;
use crate::windows::window_list::WindowList;

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
                show_cursor(cursor_for(Some(grip)));
            }
            Press::Beside => self.beside = Some(point),
            Press::Start => {
                self.selection = Selection::start(scene.monitor_rects(), point);
                self.click = windows
                    .and_then(|windows| windows.window_at(point))
                    .map(|window| Click { at: point, window });
                show_cursor(IDC_CROSS);
            }
        }
    }

    pub(super) fn hover(&mut self, scene: &Scene, point: Point) -> bool {
        if self.marks_busy() {
            self.drag_mark(point);
            return true;
        }
        if let Some(at) = self.beside {
            if !beyond_drag_threshold(at, point) {
                return false;
            }
            self.beside = None;
            self.selection = Selection::start(scene.monitor_rects(), at);
            show_cursor(IDC_CROSS);
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
                let reach = scene.reach_at(point);
                let grip = self.grip(&selection, point, reach);
                let shape = self.mark_cursor(point, grip, reach).unwrap_or_else(|| {
                    if self.purpose.reshapes() {
                        cursor_for(grip)
                    } else {
                        IDC_ARROW
                    }
                });
                show_cursor(shape);
                false
            }
            None => true,
        }
    }

    pub(super) fn release(&mut self, scene: &Scene, window: HWND, point: Point) -> bool {
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

fn beyond_drag_threshold(from: Point, to: Point) -> bool {
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
    (to.x - from.x).abs() > width.max(1) || (to.y - from.y).abs() > height.max(1)
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
}
