use bediz_core::delivery::Delivery;
use bediz_core::geometry::{Point, Rect};
use bediz_core::selection::{Grip, Handle, Modifiers, Selection};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_A, VK_C, VK_CONTROL, VK_DOWN,
    VK_ESCAPE, VK_F4, VK_LEFT, VK_MENU, VK_R, VK_RETURN, VK_RIGHT, VK_S, VK_SHIFT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, IDC_CROSS, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
    IDC_SIZEWE, LoadCursorW, MSG, SM_CXDRAG, SM_CYDRAG, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::{PCWSTR, Result};

use super::overlay::View;
use super::scene::Scene;
use super::window_class;
use super::window_list::WindowList;

pub enum Action {
    None,
    Cancel,
    Deliver(Delivery),
    CopyColour(Point),
}

struct Click {
    at: Point,
    window: Rect,
}

pub struct Session {
    previous: HWND,
    selection: Option<Selection>,
    click: Option<Click>,
    pointer: Option<Point>,
    views: Vec<View>,
}

impl Session {
    pub fn new(previous: HWND) -> Self {
        show_cursor(IDC_CROSS);
        let mut cursor = POINT::default();
        let pointer = unsafe { GetCursorPos(&mut cursor) }
            .ok()
            .map(|()| Point::new(cursor.x, cursor.y));
        Self {
            previous,
            selection: None,
            click: None,
            pointer,
            views: Vec::new(),
        }
    }

    pub fn previous(&self) -> HWND {
        self.previous
    }

    pub fn selected(&self) -> Option<Rect> {
        self.selection?.rect()
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
            WM_LBUTTONDBLCLK if self.inside_selection(scene, point) => {
                return Ok(Action::Deliver(Delivery::COPY_AND_SAVE));
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                self.press(scene, windows, point);
                if self.selection.is_some() {
                    unsafe { SetCapture(message.hwnd) };
                }
                true
            }
            WM_MOUSEMOVE => self.hover(scene, point),
            WM_LBUTTONUP => {
                let _ = unsafe { ReleaseCapture() };
                self.release(scene, point)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => match self.key(scene, last_region, message, point) {
                Some(Action::None) => true,
                Some(action) => return Ok(action),
                None => false,
            },
            WM_KEYUP | WM_SYSKEYUP => self.modifiers_changed(message),
            _ => false,
        };
        if changed {
            self.redraw(scene, windows)?;
        }
        Ok(Action::None)
    }

    fn inside_selection(&self, scene: &Scene, point: Point) -> bool {
        self.selection.is_some_and(|selection| {
            !selection.is_dragging()
                && selection.grip_at(point, scene.reach_at(point)) == Some(Grip::Inside)
        })
    }

    fn press(&mut self, scene: &Scene, windows: Option<&WindowList>, point: Point) {
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
                let grip = selection.grip_at(point, scene.reach_at(point));
                show_cursor(cursor_for(grip));
                false
            }
            None => true,
        }
    }

    fn release(&mut self, scene: &Scene, point: Point) -> bool {
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

    fn key(
        &mut self,
        scene: &Scene,
        last_region: Option<Rect>,
        message: &MSG,
        point: Point,
    ) -> Option<Action> {
        let key = VIRTUAL_KEY(message.wParam.0 as u16);
        let control = is_down(VK_CONTROL);
        let shift = is_down(VK_SHIFT);
        let step = if shift { 10 } else { 1 };
        let selected = self.selected().is_some();
        match key {
            VK_ESCAPE => Some(Action::Cancel),
            VK_F4 if message.message == WM_SYSKEYDOWN => Some(Action::Cancel),
            VK_RETURN if selected => Some(Action::Deliver(Delivery::COPY_AND_SAVE)),
            VK_C if control && selected => Some(Action::Deliver(Delivery::COPY)),
            VK_S if control && !shift && selected => Some(Action::Deliver(Delivery::SAVE)),
            VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
                let (dx, dy) = match key {
                    VK_LEFT => (-step, 0),
                    VK_RIGHT => (step, 0),
                    VK_UP => (0, -step),
                    _ => (0, step),
                };
                let selection = self.selection.as_mut()?;
                if control {
                    selection.resize_by(dx, dy);
                } else {
                    selection.nudge(dx, dy);
                }
                Some(Action::None)
            }
            VK_A if control => {
                self.click = None;
                self.selection = Selection::whole_monitor(scene.monitor_rects(), point);
                Some(Action::None)
            }
            VK_R if !control => {
                let region = last_region?;
                self.click = None;
                self.selection = Selection::restore(scene.monitor_rects(), region);
                Some(Action::None)
            }
            VK_C if !control => Some(Action::CopyColour(
                self.magnified().or(self.pointer).unwrap_or(point),
            )),
            VK_SHIFT | VK_MENU => self.modifiers_changed(message).then_some(Action::None),
            _ => None,
        }
    }

    fn modifiers_changed(&mut self, message: &MSG) -> bool {
        let key = VIRTUAL_KEY(message.wParam.0 as u16);
        match &mut self.selection {
            Some(selection) if selection.is_dragging() && (key == VK_SHIFT || key == VK_MENU) => {
                selection.change_modifiers(modifiers());
                true
            }
            _ => false,
        }
    }

    pub fn redraw(&mut self, scene: &Scene, windows: Option<&WindowList>) -> Result<()> {
        let monitors = scene.monitor_rects();
        self.views.resize(monitors.len(), View::default());
        let selection = self.selected();
        let highlight = match (&self.click, selection) {
            (Some(click), _) => Some(click.window),
            (None, None) => self
                .pointer
                .zip(windows)
                .and_then(|(pointer, windows)| windows.window_at(pointer)),
            (None, Some(_)) => None,
        };
        let labelled = selection
            .or(highlight)
            .and_then(|rect| label_monitor(monitors, rect));
        let magnified = self.magnified();
        for (index, (bounds, drawn)) in monitors.iter().zip(self.views.iter_mut()).enumerate() {
            let shown_here = |rect: &Rect| bounds.intersection(rect).is_some();
            let view = View {
                selection: selection.filter(shown_here),
                highlight: highlight.filter(shown_here),
                labelled: labelled == Some(index),
                pointer: magnified.filter(|pointer| bounds.contains(*pointer)),
            };
            if *drawn != view {
                scene.draw_one(index, &view)?;
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
