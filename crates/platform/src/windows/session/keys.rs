//! The keys of a selection: what each one does on a screenshot, a recording or an opened picture,
//! with or without a selection or the words read from it, and acting on it for the selection, the
//! marks, the words or the app.

use bediz_core::delivery::Delivery;
use bediz_core::geometry::{Point, Rect};
use bediz_core::markup::{Markup, Tool};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_ADD, VK_APPS, VK_BACK, VK_C, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_F3, VK_F4,
    VK_F10, VK_LEFT, VK_MENU, VK_O, VK_OEM_MINUS, VK_OEM_PLUS, VK_R, VK_RETURN, VK_RIGHT, VK_S,
    VK_SHIFT, VK_SUBTRACT, VK_T, VK_UP, VK_Y, VK_Z,
};

use super::{Action, KeyPress, Purpose, Session, confirm, modifiers};
use crate::windows::scene::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    selected: bool,
    words: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Response {
    Act(Action),
    Escape,
    Move { dx: i32, dy: i32, resize: bool },
    WholeMonitor,
    ChooseAllWords,
    LastRegion,
    CopyColour,
    Menu,
    Modifiers,
    Tool(Tool),
    Undo,
    Redo,
    Delete,
    Widen(i32),
}

impl Session {
    pub(super) fn key(
        &mut self,
        scene: &Scene,
        last_region: Option<Rect>,
        press: KeyPress,
        point: Point,
    ) -> Option<Action> {
        if let Some(handled) = self.typing_key(press) {
            return handled;
        }
        let held = Held {
            selected: self.selected().is_some(),
            words: self.shows_words(),
        };
        let response = respond(press, held, self.enter, self.purpose)?;
        let left_words =
            matches!(response, Response::Escape | Response::Tool(_)) && self.leave_words();
        if left_words && response == Response::Escape {
            return Some(Action::None);
        }
        let markup = self.marking.markup_mut();
        match response {
            Response::Act(action) => Some(action),
            Response::Escape => {
                if !markup.back_out() {
                    return Some(Action::Cancel);
                }
                Some(Action::None)
            }
            Response::ChooseAllWords => self.choose_all_words().then_some(Action::None),
            Response::Move { dx, dy, .. } if markup.selected().is_some() => {
                markup.nudge_selected(dx, dy);
                Some(Action::None)
            }
            Response::Move { .. } if !self.purpose.reshapes() => None,
            Response::Move { dx, dy, resize } => {
                let selection = self.selection.as_mut()?;
                if resize {
                    selection.resize_by(dx, dy);
                } else {
                    selection.nudge(dx, dy);
                }
                Some(Action::None)
            }
            Response::Tool(tool) => {
                self.marking.choose(tool);
                Some(Action::None)
            }
            Response::Undo | Response::Redo | Response::Delete => {
                edit_marks(markup, response).then_some(Action::None)
            }
            Response::Widen(steps) => {
                self.marking.widen(steps);
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

    pub(super) fn modifiers_changed(&mut self, key: VIRTUAL_KEY) -> bool {
        match &mut self.selection {
            Some(selection) if selection.is_dragging() && (key == VK_SHIFT || key == VK_MENU) => {
                selection.change_modifiers(modifiers());
                true
            }
            _ => false,
        }
    }
}

fn edit_marks(markup: &mut Markup, response: Response) -> bool {
    let revision = markup.revision();
    match response {
        Response::Undo => markup.undo(),
        Response::Redo => markup.redo(),
        _ => markup.delete_selected(),
    };
    markup.revision() != revision
}

fn respond(press: KeyPress, held: Held, enter: Delivery, purpose: Purpose) -> Option<Response> {
    let Held { selected, words } = held;
    let step = if press.shift { 10 } else { 1 };
    let moved = |dx, dy| Response::Move {
        dx,
        dy,
        resize: press.control,
    };
    let still = purpose.is_still();
    let marking = still && selected;
    let reshapes = purpose.reshapes();
    let response = match press.key {
        VK_Z if press.control && marking => {
            if press.shift {
                Response::Redo
            } else {
                Response::Undo
            }
        }
        VK_Y if press.control && marking => Response::Redo,
        VK_DELETE | VK_BACK if marking => Response::Delete,
        VK_OEM_PLUS | VK_ADD if marking => Response::Widen(1),
        VK_OEM_MINUS | VK_SUBTRACT if marking => Response::Widen(-1),
        VK_ESCAPE => Response::Escape,
        VK_F4 if press.alt => Response::Act(Action::Cancel),
        VK_RETURN if selected => Response::Act(confirm(purpose, enter)),
        VK_C if press.control && words => Response::Act(Action::CopyWords),
        VK_A if press.control && words => Response::ChooseAllWords,
        VK_C if press.control && selected && still => {
            Response::Act(Action::Deliver(Delivery::COPY))
        }
        VK_S if press.control && !press.shift && selected && still => {
            Response::Act(Action::Deliver(Delivery::SAVE))
        }
        VK_S if press.control && press.shift && selected && still => Response::Act(Action::SaveAs),
        VK_F3 if !press.control && !press.alt && !press.shift && marking && !press.repeat => {
            Response::Act(Action::Pin)
        }
        VK_T if press.control && !press.alt && !press.shift && marking && !press.repeat => {
            Response::Act(Action::Pin)
        }
        VK_LEFT => moved(-step, 0),
        VK_RIGHT => moved(step, 0),
        VK_UP => moved(0, -step),
        VK_DOWN => moved(0, step),
        VK_A if press.control && reshapes => Response::WholeMonitor,
        VK_R if !press.control && reshapes => Response::LastRegion,
        VK_C if !press.control && still => Response::CopyColour,
        VK_O if !press.control && !press.alt && !press.repeat && marking => {
            Response::Act(Action::ReadText)
        }
        VK_APPS => Response::Menu,
        VK_F10 if press.shift => Response::Menu,
        VK_SHIFT | VK_MENU => Response::Modifiers,
        _ if marking && !press.control && !press.alt => {
            return u8::try_from(press.key.0)
                .ok()
                .filter(u8::is_ascii_uppercase)
                .and_then(|key| Tool::for_key(char::from(key)))
                .map(Response::Tool);
        }
        _ => return None,
    };
    Some(response)
}

#[cfg(test)]
mod tests {
    use bediz_core::delivery::Delivery;
    use bediz_core::geometry::Point;
    use bediz_core::markup::{Markup, Swatch, Tool};
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VIRTUAL_KEY, VK_A, VK_ADD, VK_C,
        VK_DELETE, VK_ESCAPE, VK_F3, VK_F4, VK_LEFT, VK_NUMPAD1, VK_O, VK_OEM_MINUS, VK_P, VK_R,
        VK_RETURN, VK_S, VK_T, VK_Y, VK_Z,
    };

    use super::{Action, Held, KeyPress, Purpose, Response, edit_marks, respond};
    use crate::windows::hotkey::as_key_press;

    fn pressed(
        purpose: Purpose,
        modifiers: HOT_KEY_MODIFIERS,
        key: VIRTUAL_KEY,
        held: Held,
    ) -> Option<Response> {
        let keys = LPARAM(((key.0 as isize) << 16) | modifiers.0 as isize);
        respond(as_key_press(keys), held, Delivery::COPY_AND_SAVE, purpose)
    }

    fn replayed_for(
        purpose: Purpose,
        modifiers: HOT_KEY_MODIFIERS,
        key: VIRTUAL_KEY,
        selected: bool,
    ) -> Option<Response> {
        let held = Held {
            selected,
            words: false,
        };
        pressed(purpose, modifiers, key, held)
    }

    fn replayed(
        modifiers: HOT_KEY_MODIFIERS,
        key: VIRTUAL_KEY,
        selected: bool,
    ) -> Option<Response> {
        replayed_for(Purpose::Screenshot, modifiers, key, selected)
    }

    #[test]
    fn enter_starts_a_recording_and_screenshot_keys_do_nothing_there() {
        let recording = |modifiers, key| replayed_for(Purpose::Recording, modifiers, key, true);
        assert_eq!(
            recording(HOT_KEY_MODIFIERS(0), VK_RETURN),
            Some(Response::Act(Action::Record))
        );
        assert_eq!(recording(MOD_CONTROL, VK_C), None);
        assert_eq!(recording(MOD_CONTROL, VK_S), None);
        assert_eq!(recording(MOD_CONTROL | MOD_SHIFT, VK_S), None);
        assert_eq!(recording(HOT_KEY_MODIFIERS(0), VK_C), None);
        assert_eq!(recording(MOD_CONTROL, VK_A), Some(Response::WholeMonitor));
        assert_eq!(
            recording(HOT_KEY_MODIFIERS(0), VK_ESCAPE),
            Some(Response::Escape)
        );
        assert_eq!(
            replayed_for(Purpose::Recording, HOT_KEY_MODIFIERS(0), VK_RETURN, false),
            None
        );
    }

    #[test]
    fn enter_in_a_screenshot_does_what_the_setting_says() {
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_RETURN, true),
            Some(Response::Act(Action::Deliver(Delivery::COPY_AND_SAVE)))
        );
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
    fn letters_choose_tools_on_a_screenshot_selection_only() {
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_A, true),
            Some(Response::Tool(Tool::Arrow))
        );
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_T, true),
            Some(Response::Tool(Tool::Text))
        );
        assert_eq!(replayed(HOT_KEY_MODIFIERS(0), VK_A, false), None);
        assert_eq!(
            replayed_for(Purpose::Recording, HOT_KEY_MODIFIERS(0), VK_P, true),
            None
        );
        assert_eq!(replayed(MOD_CONTROL, VK_P, true), None);
        assert_eq!(replayed(HOT_KEY_MODIFIERS(0), VK_NUMPAD1, true), None);
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_R, true),
            Some(Response::LastRegion)
        );
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_C, true),
            Some(Response::CopyColour)
        );
    }

    #[test]
    fn a_picture_takes_the_screenshot_keys_but_keeps_its_selection() {
        let picture = |modifiers, key| replayed_for(Purpose::Picture, modifiers, key, true);
        assert_eq!(
            picture(HOT_KEY_MODIFIERS(0), VK_RETURN),
            Some(Response::Act(Action::Deliver(Delivery::COPY_AND_SAVE)))
        );
        assert_eq!(
            picture(MOD_CONTROL, VK_C),
            Some(Response::Act(Action::Deliver(Delivery::COPY)))
        );
        assert_eq!(
            picture(MOD_CONTROL | MOD_SHIFT, VK_S),
            Some(Response::Act(Action::SaveAs))
        );
        assert_eq!(picture(MOD_CONTROL, VK_Z), Some(Response::Undo));
        assert_eq!(
            picture(HOT_KEY_MODIFIERS(0), VK_P),
            Some(Response::Tool(Tool::Pen))
        );
        assert_eq!(
            picture(HOT_KEY_MODIFIERS(0), VK_C),
            Some(Response::CopyColour)
        );
        assert_eq!(picture(MOD_CONTROL, VK_A), None);
        assert_eq!(picture(HOT_KEY_MODIFIERS(0), VK_R), None);
        assert_eq!(
            picture(HOT_KEY_MODIFIERS(0), VK_O),
            Some(Response::Act(Action::ReadText))
        );
    }

    #[test]
    fn o_reads_the_text_of_a_screenshot_or_picture_selection_only() {
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_O, true),
            Some(Response::Act(Action::ReadText))
        );
        assert_eq!(replayed(HOT_KEY_MODIFIERS(0), VK_O, false), None);
        assert_eq!(replayed(MOD_CONTROL, VK_O, true), None);
        assert_eq!(replayed(MOD_ALT, VK_O, true), None);
        assert_eq!(
            replayed_for(Purpose::Recording, HOT_KEY_MODIFIERS(0), VK_O, true),
            None
        );
        let held = KeyPress {
            repeat: true,
            ..as_key_press(LPARAM((VK_O.0 as isize) << 16))
        };
        let selected = Held {
            selected: true,
            words: false,
        };
        assert_eq!(
            respond(held, selected, Delivery::COPY_AND_SAVE, Purpose::Screenshot),
            None
        );
    }

    #[test]
    fn with_words_shown_ctrl_c_copies_them_and_ctrl_a_chooses_them_all() {
        let words = Held {
            selected: true,
            words: true,
        };
        for purpose in [Purpose::Screenshot, Purpose::Picture] {
            let key = |modifiers, key| pressed(purpose, modifiers, key, words);
            assert_eq!(
                key(MOD_CONTROL, VK_C),
                Some(Response::Act(Action::CopyWords))
            );
            assert_eq!(key(MOD_CONTROL, VK_A), Some(Response::ChooseAllWords));
            assert_eq!(key(HOT_KEY_MODIFIERS(0), VK_ESCAPE), Some(Response::Escape));
            assert_eq!(
                key(HOT_KEY_MODIFIERS(0), VK_P),
                Some(Response::Tool(Tool::Pen))
            );
            assert_eq!(
                key(HOT_KEY_MODIFIERS(0), VK_O),
                Some(Response::Act(Action::ReadText))
            );
            assert_eq!(
                key(MOD_CONTROL, VK_S),
                Some(Response::Act(Action::Deliver(Delivery::SAVE)))
            );
            assert_eq!(key(HOT_KEY_MODIFIERS(0), VK_C), Some(Response::CopyColour));
        }
    }

    #[test]
    fn f3_or_ctrl_t_pins_a_still_selection_and_t_alone_still_chooses_the_text_tool() {
        for purpose in [Purpose::Screenshot, Purpose::Picture] {
            let key = |modifiers, key| replayed_for(purpose, modifiers, key, true);
            assert_eq!(
                key(HOT_KEY_MODIFIERS(0), VK_F3),
                Some(Response::Act(Action::Pin))
            );
            assert_eq!(key(MOD_CONTROL, VK_T), Some(Response::Act(Action::Pin)));
            assert_eq!(
                key(HOT_KEY_MODIFIERS(0), VK_T),
                Some(Response::Tool(Tool::Text))
            );
            for modifiers in [MOD_CONTROL, MOD_ALT, MOD_SHIFT, MOD_CONTROL | MOD_SHIFT] {
                assert_eq!(key(modifiers, VK_F3), None);
            }
            assert_eq!(key(MOD_CONTROL | MOD_SHIFT, VK_T), None);
            assert_eq!(key(MOD_CONTROL | MOD_ALT, VK_T), None);
            assert_eq!(key(MOD_ALT, VK_T), None);
            assert_eq!(key(MOD_SHIFT, VK_T), Some(Response::Tool(Tool::Text)));
        }
        assert_eq!(replayed(HOT_KEY_MODIFIERS(0), VK_F3, false), None);
        assert_eq!(
            replayed_for(Purpose::Recording, HOT_KEY_MODIFIERS(0), VK_F3, true),
            None
        );
        let held = KeyPress {
            repeat: true,
            ..as_key_press(LPARAM((VK_F3.0 as isize) << 16))
        };
        let selected = Held {
            selected: true,
            words: false,
        };
        assert_eq!(
            respond(held, selected, Delivery::COPY_AND_SAVE, Purpose::Screenshot),
            None
        );
    }

    #[test]
    fn undo_redo_delete_and_width_keys_act_on_marks() {
        assert_eq!(replayed(MOD_CONTROL, VK_Z, true), Some(Response::Undo));
        assert_eq!(
            replayed(MOD_CONTROL | MOD_SHIFT, VK_Z, true),
            Some(Response::Redo)
        );
        assert_eq!(replayed(MOD_CONTROL, VK_Y, true), Some(Response::Redo));
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_DELETE, true),
            Some(Response::Delete)
        );
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_ADD, true),
            Some(Response::Widen(1))
        );
        assert_eq!(
            replayed(HOT_KEY_MODIFIERS(0), VK_OEM_MINUS, true),
            Some(Response::Widen(-1))
        );
        assert_eq!(replayed(MOD_CONTROL, VK_Z, false), None);
        assert_eq!(
            replayed_for(Purpose::Recording, MOD_CONTROL, VK_Z, true),
            None
        );
    }

    #[test]
    fn undo_or_redo_while_the_first_mark_is_drawn_drops_it_and_asks_for_a_redraw() {
        for response in [Response::Undo, Response::Redo] {
            let mut markup = Markup::default();
            markup.draw(
                Tool::Rectangle,
                Point::new(10, 10),
                Swatch::Red.colour(),
                3.0,
            );
            markup.drag(Point::new(60, 40), false);
            assert!(edit_marks(&mut markup, response), "{response:?}");
            assert!(markup.is_empty());
            assert!(!markup.is_busy());
            assert!(!edit_marks(&mut markup, response), "{response:?}");
            assert!(!edit_marks(&mut markup, Response::Delete));
        }
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
