use bediz_core::delivery::Delivery;
use bediz_core::geometry::{Point, Rect};
use bediz_core::selection::{Grip, Handle, Modifiers, Selection};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::SystemServices::MK_LBUTTON;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_A, VK_APPS, VK_C, VK_CONTROL, VK_DOWN,
    VK_ESCAPE, VK_F4, VK_F10, VK_LEFT, VK_MENU, VK_R, VK_RETURN, VK_RIGHT, VK_S, VK_SHIFT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, IDC_CROSS, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
    IDC_SIZEWE, LoadCursorW, MSG, SM_CXDRAG, SM_CYDRAG, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::{PCWSTR, Result};

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
    CopyColour(Point),
    Menu(Point),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: VIRTUAL_KEY,
    pub alt: bool,
    pub control: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Response {
    Act(Action),
    Move { dx: i32, dy: i32, resize: bool },
    WholeMonitor,
    LastRegion,
    CopyColour,
    Menu,
    Modifiers,
}

struct Click {
    at: Point,
    window: Rect,
}

pub struct Session {
    previous: HWND,
    enter: Delivery,
    selection: Option<Selection>,
    click: Option<Click>,
    pointer: Option<Point>,
    views: Vec<View>,
}

impl Session {
    pub fn new(previous: HWND, enter: Delivery) -> Self {
        show_cursor(IDC_CROSS);
        let mut cursor = POINT::default();
        let pointer = unsafe { GetCursorPos(&mut cursor) }
            .ok()
            .map(|()| Point::new(cursor.x, cursor.y));
        Self {
            previous,
            enter,
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
                return Ok(Action::Deliver(self.enter));
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                self.press(scene, windows, point);
                if self.selection.is_some() {
                    unsafe { SetCapture(message.hwnd) };
                }
                true
            }
            // Alt+Tab or the Win key can take the mouse mid-drag; the button then comes up elsewhere.
            WM_MOUSEMOVE
                if message.wParam.0 & MK_LBUTTON.0 as usize == 0
                    && (self.click.is_some() || self.is_dragging()) =>
            {
                let _ = unsafe { ReleaseCapture() };
                self.release(scene, point)
            }
            WM_MOUSEMOVE => self.hover(scene, point),
            WM_LBUTTONUP => {
                let _ = unsafe { ReleaseCapture() };
                self.release(scene, point)
            }
            WM_RBUTTONUP if !self.is_dragging() => return Ok(Action::Menu(point)),
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
        press: KeyPress,
        point: Point,
    ) -> Option<Action> {
        match respond(press, self.selected().is_some(), self.enter)? {
            Response::Act(action) => Some(action),
            Response::Move { dx, dy, resize } => {
                let selection = self.selection.as_mut()?;
                if resize {
                    selection.resize_by(dx, dy);
                } else {
                    selection.nudge(dx, dy);
                }
                Some(Action::None)
            }
            Response::WholeMonitor => {
                self.select_whole_monitor(scene, point);
                Some(Action::None)
            }
            Response::LastRegion => {
                self.select_region(scene, last_region?);
                Some(Action::None)
            }
            Response::CopyColour => Some(Action::CopyColour(
                self.magnified().or(self.pointer).unwrap_or(point),
            )),
            Response::Menu => Some(Action::Menu(self.pointer.unwrap_or(point))),
            Response::Modifiers => self.modifiers_changed(press.key).then_some(Action::None),
        }
    }

    fn modifiers_changed(&mut self, key: VIRTUAL_KEY) -> bool {
        match &mut self.selection {
            Some(selection) if selection.is_dragging() && (key == VK_SHIFT || key == VK_MENU) => {
                selection.change_modifiers(modifiers());
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

fn respond(press: KeyPress, selected: bool, enter: Delivery) -> Option<Response> {
    let step = if press.shift { 10 } else { 1 };
    let moved = |dx, dy| Response::Move {
        dx,
        dy,
        resize: press.control,
    };
    let response = match press.key {
        VK_ESCAPE => Response::Act(Action::Cancel),
        VK_F4 if press.alt => Response::Act(Action::Cancel),
        VK_RETURN if selected => Response::Act(Action::Deliver(enter)),
        VK_C if press.control && selected => Response::Act(Action::Deliver(Delivery::COPY)),
        VK_S if press.control && !press.shift && selected => {
            Response::Act(Action::Deliver(Delivery::SAVE))
        }
        VK_S if press.control && press.shift && selected => Response::Act(Action::SaveAs),
        VK_LEFT => moved(-step, 0),
        VK_RIGHT => moved(step, 0),
        VK_UP => moved(0, -step),
        VK_DOWN => moved(0, step),
        VK_A if press.control => Response::WholeMonitor,
        VK_R if !press.control => Response::LastRegion,
        VK_C if !press.control => Response::CopyColour,
        VK_APPS => Response::Menu,
        VK_F10 if press.shift => Response::Menu,
        VK_SHIFT | VK_MENU => Response::Modifiers,
        _ => return None,
    };
    Some(response)
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
    use bediz_core::delivery::Delivery;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VIRTUAL_KEY, VK_C, VK_F4, VK_LEFT, VK_S,
    };

    use super::{Action, Response, respond};
    use crate::windows::hotkey::as_key_press;

    fn replayed(
        modifiers: HOT_KEY_MODIFIERS,
        key: VIRTUAL_KEY,
        selected: bool,
    ) -> Option<Response> {
        let keys = LPARAM(((key.0 as isize) << 16) | modifiers.0 as isize);
        respond(as_key_press(keys), selected, Delivery::COPY_AND_SAVE)
    }

    #[test]
    fn a_shortcut_replayed_after_its_keys_came_up_acts_as_it_was_pressed() {
        assert_eq!(
            replayed(MOD_CONTROL, VK_C, true),
            Some(Response::Act(Action::Deliver(Delivery::COPY)))
        );
        assert_eq!(
            replayed(MOD_CONTROL, VK_S, true),
            Some(Response::Act(Action::Deliver(Delivery::SAVE)))
        );
        assert_eq!(
            replayed(MOD_CONTROL | MOD_SHIFT, VK_S, true),
            Some(Response::Act(Action::SaveAs))
        );
        assert_eq!(
            replayed(MOD_ALT, VK_F4, true),
            Some(Response::Act(Action::Cancel))
        );
        assert_eq!(
            replayed(MOD_CONTROL | MOD_SHIFT, VK_LEFT, true),
            Some(Response::Move {
                dx: -10,
                dy: 0,
                resize: true
            })
        );
    }

    #[test]
    fn a_key_without_its_modifier_does_what_the_bare_key_does() {
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_C, true),
            Some(Response::CopyColour)
        );
        assert_eq!(replayed(HOT_KEY_MODIFIERS(0), VK_S, true), None);
        assert_eq!(replayed(MOD_CONTROL, VK_C, false), None);
    }
}
