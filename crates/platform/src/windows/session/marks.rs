//! The marking-up side of a selection's input: presses that draw or grab marks, dragging and
//! letting them go, the pointer's shape over them, typing in a text with or without an input
//! method, moving its caret, and the wheel that changes the width.

use bediz_core::geometry::Point;
use bediz_core::markup::typing::Motion;
use bediz_core::markup::{Shape, Tool, shape};
use bediz_core::selection::Grip;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_BACK, VK_C, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F4, VK_HOME,
    VK_LEFT, VK_MENU, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_UP, VK_V, VK_X, VK_Y, VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IDC_CROSS, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE,
    WHEEL_DELTA,
};
use windows::core::PCWSTR;

use super::{Action, KeyPress, Session, is_down};
use crate::windows::ime::Update;
use crate::windows::marking::layout::Typed;
use crate::windows::scene::Scene;

impl Session {
    pub fn is_typing(&self) -> bool {
        self.marking.markup().editing().is_some()
    }

    pub(super) fn marks_busy(&self) -> bool {
        self.marking.markup().is_busy()
    }

    pub(super) fn press_mark(&mut self, scene: &Scene, window: HWND, point: Point) -> bool {
        self.marking.markup_mut().settle_composition();
        if !self.purpose.is_still() {
            return false;
        }
        let Some(selection) = self.selection.filter(|selection| !selection.is_dragging()) else {
            return false;
        };
        let Some(rect) = selection.rect() else {
            return false;
        };
        let reach = scene.reach_at(point);
        let on_handle = matches!(self.grip(&selection, point, reach), Some(Grip::Handle(_)));
        let on_corner = self
            .marking
            .markup()
            .selected()
            .is_some_and(|mark| shape::corner_at(mark, point, reach).is_some());
        if on_handle && !on_corner {
            return false;
        }
        let Some(tool) = self.marking.tool() else {
            return self.marking.markup_mut().grab(point, reach, rect);
        };
        if self.marking.markup_mut().grab_selected(point, reach) || !rect.contains(point) {
            return true;
        }
        let text = (tool == Tool::Text)
            .then(|| self.marking.markup().text_at(point, reach))
            .flatten();
        match text {
            Some(id) => {
                self.marking.markup_mut().edit_text(id);
                self.caret_to(scene, point);
            }
            None => self.marking.draw(tool, point),
        }
        self.marking.place_composition(scene, window);
        true
    }

    pub(super) fn drag_mark(&mut self, point: Point) {
        self.marking.markup_mut().drag(point, is_down(VK_SHIFT));
    }

    pub(super) fn release_mark(&mut self, scene: &Scene, window: HWND, point: Point) {
        let changed = self.marking.markup_mut().finish(point, is_down(VK_SHIFT));
        let text = self
            .marking
            .markup()
            .selected()
            .filter(|mark| matches!(mark.shape, Shape::Text { .. }))
            .map(|mark| mark.id);
        if !changed
            && self.marking.tool() == Some(Tool::Text)
            && let Some(id) = text
        {
            self.marking.markup_mut().edit_text(id);
            self.caret_to(scene, point);
            self.marking.place_composition(scene, window);
        }
    }

    pub(super) fn mark_cursor(
        &self,
        point: Point,
        grip: Option<Grip>,
        reach: i32,
    ) -> Option<PCWSTR> {
        if !self.purpose.is_still() {
            return None;
        }
        let selected = self.marking.markup().selected();
        if let Some(facing) = selected.and_then(|mark| shape::corner_facing(mark, point, reach)) {
            return Some(facing_cursor(facing));
        }
        if matches!(grip, Some(Grip::Handle(_))) {
            return None;
        }
        let tool = self.marking.tool()?;
        if selected.is_some_and(|mark| shape::outline_touches(mark, point, reach)) {
            return Some(IDC_SIZEALL);
        }
        if !self.selected().is_some_and(|rect| rect.contains(point)) {
            return None;
        }
        Some(if tool == Tool::Text {
            IDC_IBEAM
        } else {
            IDC_CROSS
        })
    }

    pub(super) fn type_unit(&mut self, scene: &Scene, window: HWND, unit: u16) -> bool {
        if !self.is_typing() {
            return false;
        }
        self.marking.type_unit(unit);
        self.marking.measure_texts(scene);
        self.marking.place_composition(scene, window);
        true
    }

    pub fn hear_input_method(&mut self, scene: &Scene, window: HWND, updates: Vec<Update>) {
        self.marking.hear_input_method(updates);
        self.marking.measure_texts(scene);
        self.marking.place_composition(scene, window);
    }

    pub(super) fn turn_wheel(&mut self, delta: i16) -> bool {
        if !self.purpose.is_still() || self.selected().is_none() {
            return false;
        }
        let steps = i32::from(delta) / WHEEL_DELTA as i32;
        if steps == 0 {
            return false;
        }
        self.marking.widen(steps);
        true
    }

    pub(super) fn typing_key(&mut self, press: KeyPress) -> Option<Option<Action>> {
        if !self.is_typing() {
            return None;
        }
        // AltGr reaches the app as Ctrl+Alt, and its letters are characters, not shortcuts.
        let shortcut = press.control && !press.alt && !is_down(VK_MENU);
        let markup = self.marking.markup_mut();
        let handled = match press.key {
            VK_ESCAPE => {
                markup.end_editing();
                Action::None
            }
            VK_RETURN if !press.shift => {
                markup.end_editing();
                Action::None
            }
            VK_RETURN => Action::None,
            VK_BACK | VK_DELETE => {
                markup.erase(press.key == VK_DELETE, press.control);
                Action::None
            }
            VK_A if shortcut => {
                markup.select_all_text();
                Action::None
            }
            VK_C if shortcut && markup.selected_text().is_some() => Action::CopyText { cut: false },
            VK_X if shortcut => match markup.selected_text() {
                Some(_) => Action::CopyText { cut: true },
                None => return Some(None),
            },
            VK_V if shortcut => Action::PasteText,
            VK_Z if shortcut && press.shift => {
                markup.redo();
                Action::None
            }
            VK_Z if shortcut => {
                markup.undo();
                Action::None
            }
            VK_Y if shortcut => {
                markup.redo();
                Action::None
            }
            VK_F4 if press.alt => return None,
            key => match motion_for(key, press.control) {
                Some(motion) => {
                    markup.move_caret(motion, press.shift);
                    Action::None
                }
                None if shortcut => return None,
                None => return Some(None),
            },
        };
        Some(Some(handled))
    }

    fn caret_to(&mut self, scene: &Scene, point: Point) {
        let index = self
            .marking
            .markup()
            .editing()
            .zip(scene.gpu_near(point).ok())
            .and_then(|(mark, gpu)| Typed::of(&gpu.text, mark))
            .and_then(|typed| typed.index_at(point));
        if let Some(index) = index {
            self.marking.markup_mut().place_caret(index);
        }
    }
}

fn motion_for(key: VIRTUAL_KEY, control: bool) -> Option<Motion> {
    Some(match (key, control) {
        (VK_LEFT, false) => Motion::Left,
        (VK_LEFT, true) => Motion::WordLeft,
        (VK_RIGHT, false) => Motion::Right,
        (VK_RIGHT, true) => Motion::WordRight,
        (VK_UP, _) => Motion::Up,
        (VK_DOWN, _) => Motion::Down,
        (VK_HOME, false) => Motion::LineStart,
        (VK_HOME, true) => Motion::Start,
        (VK_END, false) => Motion::LineEnd,
        (VK_END, true) => Motion::End,
        _ => return None,
    })
}

fn facing_cursor((x, y): (i32, i32)) -> PCWSTR {
    match (x, y) {
        (0, 0) => IDC_SIZEALL,
        (0, _) => IDC_SIZENS,
        (_, 0) => IDC_SIZEWE,
        _ if x == y => IDC_SIZENWSE,
        _ => IDC_SIZENESW,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_shows_the_arrow_of_the_way_it_moves() {
        assert_eq!(facing_cursor((-1, -1)), IDC_SIZENWSE);
        assert_eq!(facing_cursor((1, 1)), IDC_SIZENWSE);
        assert_eq!(facing_cursor((1, -1)), IDC_SIZENESW);
        assert_eq!(facing_cursor((-1, 0)), IDC_SIZEWE);
        assert_eq!(facing_cursor((0, 1)), IDC_SIZENS);
        assert_eq!(facing_cursor((0, 0)), IDC_SIZEALL);
    }

    #[test]
    fn arrows_home_and_end_move_the_caret_and_ctrl_makes_them_go_further() {
        assert_eq!(motion_for(VK_LEFT, false), Some(Motion::Left));
        assert_eq!(motion_for(VK_LEFT, true), Some(Motion::WordLeft));
        assert_eq!(motion_for(VK_RIGHT, true), Some(Motion::WordRight));
        assert_eq!(motion_for(VK_DOWN, true), Some(Motion::Down));
        assert_eq!(motion_for(VK_HOME, false), Some(Motion::LineStart));
        assert_eq!(motion_for(VK_HOME, true), Some(Motion::Start));
        assert_eq!(motion_for(VK_END, true), Some(Motion::End));
        assert_eq!(motion_for(VK_A, false), None);
    }
}
