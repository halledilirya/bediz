//! One selection on the overlays, for a screenshot, a recording or an opened picture: turns the
//! mouse and keys into the selection, its marks, the window under the pointer and the action
//! chosen, and redraws.

mod keys;
mod marks;
mod picture;

pub use picture::Picture;

use bediz_core::delivery::Delivery;
use bediz_core::geometry::{Point, Rect};
use bediz_core::selection::{Grip, Handle, Modifiers, Selection};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::SystemServices::MK_LBUTTON;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, IDC_ARROW, IDC_CROSS, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS,
    IDC_SIZENWSE, IDC_SIZEWE, LoadCursorW, MSG, SM_CXDRAG, SM_CYDRAG, WM_CHAR, WM_KEYDOWN,
    WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::{PCWSTR, Result};

use super::marking::Marking;
use super::overlay::View;
use super::scene::Scene;
use super::window_class;
use super::window_list::WindowList;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Cancel,
    Deliver(Delivery),
    SaveAs,
    Record,
    CopyColour(Point),
    Menu(Point),
    CopyText { cut: bool },
    PasteText,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Screenshot,
    Recording,
    Picture,
}

impl Purpose {
    pub fn is_still(self) -> bool {
        self != Self::Recording
    }

    pub fn shows_live_screen(self) -> bool {
        self != Self::Screenshot
    }

    pub fn reshapes(self) -> bool {
        self != Self::Picture
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: VIRTUAL_KEY,
    pub alt: bool,
    pub control: bool,
    pub shift: bool,
}

struct Click {
    at: Point,
    window: Rect,
}

pub struct Session {
    previous: HWND,
    enter: Delivery,
    purpose: Purpose,
    selection: Option<Selection>,
    click: Option<Click>,
    pointer: Option<Point>,
    views: Vec<View>,
    marking: Marking,
    picture: Option<Picture>,
}

impl Session {
    pub fn new(
        previous: HWND,
        enter: Delivery,
        purpose: Purpose,
        scene: &Scene,
        reply_to: HWND,
    ) -> Self {
        show_cursor(IDC_CROSS);
        let mut cursor = POINT::default();
        let pointer = unsafe { GetCursorPos(&mut cursor) }
            .ok()
            .map(|()| Point::new(cursor.x, cursor.y));
        let scale = pointer.map_or(1.0, |pointer| scene.scale_at(pointer));
        Self {
            previous,
            enter,
            purpose,
            selection: None,
            click: None,
            pointer,
            views: Vec::new(),
            marking: Marking::new(scale, reply_to),
            picture: None,
        }
    }

    pub fn marking(&self) -> &Marking {
        &self.marking
    }

    pub fn marking_mut(&mut self) -> &mut Marking {
        &mut self.marking
    }

    pub fn previous(&self) -> HWND {
        self.previous
    }

    pub fn set_previous(&mut self, previous: HWND) {
        self.previous = previous;
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    pub fn repaint(&self, scene: &Scene, monitors: &[usize]) -> Result<()> {
        let decor = self.marking.decor();
        for &index in monitors {
            if let Some(view) = self.views.get(index) {
                scene.draw_one(index, view, &decor)?;
            }
        }
        Ok(())
    }

    pub fn selected(&self) -> Option<Rect> {
        self.selection?.rect()
    }

    pub fn settled(&self) -> Option<Rect> {
        if self.click.is_some() || self.is_dragging() {
            return None;
        }
        self.selected()
    }

    pub fn handle(
        &mut self,
        scene: &Scene,
        windows: Option<&WindowList>,
        last_region: Option<Rect>,
        message: &MSG,
    ) -> Result<Action> {
        let point = Point::new(message.pt.x, message.pt.y);
        if message.message == WM_MOUSEMOVE {
            self.pointer = Some(point);
        }
        let changed = match message.message {
            WM_LBUTTONDBLCLK
                if self.marking.tool().is_none() && self.inside_selection(scene, point) =>
            {
                return Ok(confirm(self.purpose, self.enter));
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                if !self.press_mark(scene, message.hwnd, point) {
                    self.press(scene, windows, point);
                }
                if self.selection.is_some() {
                    unsafe { SetCapture(message.hwnd) };
                }
                true
            }
            // Alt+Tab or the Win key can take the mouse mid-drag; the button then comes up elsewhere.
            WM_MOUSEMOVE
                if message.wParam.0 & MK_LBUTTON.0 as usize == 0
                    && (self.click.is_some() || self.is_dragging() || self.marks_busy()) =>
            {
                let _ = unsafe { ReleaseCapture() };
                self.release(scene, message.hwnd, point)
            }
            WM_MOUSEMOVE => self.hover(scene, point),
            WM_LBUTTONUP => {
                let _ = unsafe { ReleaseCapture() };
                self.release(scene, message.hwnd, point)
            }
            WM_RBUTTONUP if !self.is_dragging() && !self.marks_busy() => {
                return Ok(Action::Menu(point));
            }
            WM_CHAR => self.type_unit(scene, message.hwnd, message.wParam.0 as u16),
            WM_MOUSEWHEEL => self.turn_wheel((message.wParam.0 >> 16) as u16 as i16),
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let press = KeyPress {
                    key: VIRTUAL_KEY(message.wParam.0 as u16),
                    alt: message.message == WM_SYSKEYDOWN,
                    control: is_down(VK_CONTROL),
                    shift: is_down(VK_SHIFT),
                };
                return self.press_key(scene, windows, last_region, press, point);
            }
            WM_KEYUP | WM_SYSKEYUP => self.modifiers_changed(VIRTUAL_KEY(message.wParam.0 as u16)),
            _ => false,
        };
        if changed {
            self.redraw(scene, windows)?;
        }
        Ok(Action::None)
    }

    pub fn press_key(
        &mut self,
        scene: &Scene,
        windows: Option<&WindowList>,
        last_region: Option<Rect>,
        press: KeyPress,
        point: Point,
    ) -> Result<Action> {
        match self.key(scene, last_region, press, point) {
            Some(Action::None) => self.redraw(scene, windows).map(|()| Action::None),
            Some(action) => Ok(action),
            None => Ok(Action::None),
        }
    }

    pub fn select_whole_monitor(&mut self, scene: &Scene, at: Point) {
        self.click = None;
        self.selection = Selection::whole_monitor(scene.monitor_rects(), at);
    }

    pub fn select_region(&mut self, scene: &Scene, region: Rect) {
        self.click = None;
        self.selection = Selection::restore(scene.monitor_rects(), region);
    }

    fn is_dragging(&self) -> bool {
        self.selection
            .is_some_and(|selection| selection.is_dragging())
    }

    fn inside_selection(&self, scene: &Scene, point: Point) -> bool {
        self.selection.is_some_and(|selection| {
            !selection.is_dragging()
                && self.grip(&selection, point, scene.reach_at(point)) == Some(Grip::Inside)
        })
    }

    fn press(&mut self, scene: &Scene, windows: Option<&WindowList>, point: Point) {
        if !self.purpose.reshapes() {
            return;
        }
        if let Some(selection) = &mut self.selection
            && let Some(grip) = selection.grip_at(point, scene.reach_at(point))
        {
            selection.grab(grip, point);
            show_cursor(cursor_for(Some(grip)));
            return;
        }
        self.selection = Selection::start(scene.monitor_rects(), point);
        self.click = windows
            .and_then(|windows| windows.window_at(point))
            .map(|window| Click { at: point, window });
        show_cursor(IDC_CROSS);
    }

    fn hover(&mut self, scene: &Scene, point: Point) -> bool {
        if self.marks_busy() {
            self.drag_mark(point);
            return true;
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

    fn release(&mut self, scene: &Scene, window: HWND, point: Point) -> bool {
        if self.marks_busy() {
            self.release_mark(scene, window, point);
            return true;
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

    pub fn redraw_all(&mut self, scene: &Scene, windows: Option<&WindowList>) -> Result<()> {
        self.views.clear();
        self.redraw(scene, windows)
    }

    pub fn redraw(&mut self, scene: &Scene, windows: Option<&WindowList>) -> Result<()> {
        self.marking.measure_texts(scene);
        self.marking.refresh_previews(scene);
        let monitors = scene.monitor_rects();
        self.views.resize(monitors.len(), View::default());
        let decor = self.marking.decor();
        let marks = self.marking.version();
        let selection = self.selected();
        let highlight = match (&self.click, selection) {
            (Some(click), _) => Some(click.window),
            (None, None) => self
                .pointer
                .zip(windows)
                .and_then(|(pointer, windows)| windows.window_at(pointer)),
            (None, Some(_)) => None,
        };
        let labelled = selection.or(highlight).and_then(|rect| {
            label_monitor(monitors, rect).map(|monitor| (monitor, self.size_of(rect)))
        });
        let magnified = self.magnified();
        for (index, (bounds, drawn)) in monitors.iter().zip(self.views.iter_mut()).enumerate() {
            let shown_here = |rect: &Rect| bounds.intersection(rect).is_some();
            let view = View {
                selection: selection.filter(shown_here),
                handles: self.purpose.reshapes(),
                highlight: highlight.filter(shown_here),
                label: labelled
                    .filter(|(monitor, _)| *monitor == index)
                    .map(|(_, size)| size),
                pointer: magnified.filter(|pointer| bounds.contains(*pointer)),
                marks,
            };
            if *drawn != view {
                scene.draw_one(index, &view, &decor)?;
                *drawn = view;
            }
        }
        Ok(())
    }

    fn magnified(&self) -> Option<Point> {
        match &self.selection {
            None => self.pointer,
            Some(selection) => selection.focus(),
        }
    }
}

fn confirm(purpose: Purpose, enter: Delivery) -> Action {
    match purpose {
        Purpose::Screenshot | Purpose::Picture => Action::Deliver(enter),
        Purpose::Recording => Action::Record,
    }
}

fn label_monitor(monitors: &[Rect], rect: Rect) -> Option<usize> {
    let corner = Point::new(rect.left, rect.top);
    monitors
        .iter()
        .position(|monitor| monitor.contains(corner))
        .or_else(|| {
            monitors
                .iter()
                .position(|monitor| monitor.intersection(&rect).is_some())
        })
}

fn beyond_drag_threshold(from: Point, to: Point) -> bool {
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
    (to.x - from.x).abs() > width.max(1) || (to.y - from.y).abs() > height.max(1)
}

fn modifiers() -> Modifiers {
    Modifiers {
        square: is_down(VK_SHIFT),
        centered: is_down(VK_MENU),
    }
}

fn is_down(key: VIRTUAL_KEY) -> bool {
    let state = unsafe { GetKeyState(i32::from(key.0)) };
    state < 0
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

fn show_cursor(shape: PCWSTR) {
    if let Ok(cursor) = unsafe { LoadCursorW(None, shape) } {
        window_class::use_cursor(cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_opened_picture_keeps_its_selection_and_shows_it_without_handles() {
        assert!(Purpose::Screenshot.reshapes());
        assert!(Purpose::Recording.reshapes());
        assert!(!Purpose::Picture.reshapes());
    }
}
