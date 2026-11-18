//! The text being typed while an input method composes: the input method's updates applied, the
//! composition kept in the marks copied or saved, and where its candidate list goes.

use std::borrow::Cow;

use bediz_core::geometry::Point;
use bediz_core::markup::Mark;
use bediz_core::markup::composition::Clause;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use super::Marking;
use super::layout::Typed;
use crate::windows::ime::{self, Update};
use crate::windows::scene::Scene;

impl Marking {
    pub fn hear_input_method(&mut self, updates: Vec<Update>) {
        for update in updates {
            match update {
                Update::Finished(text) => {
                    self.markup.finish_composing(&text);
                    self.composing = false;
                }
                Update::Composing(composition) => {
                    self.composing = composition.is_some();
                    self.markup.compose(composition);
                }
            }
        }
    }

    // Once the text took the composition itself, the input method still holds it and must drop it.
    pub fn take_settled(&mut self) -> bool {
        let settled = self.composing && self.markup.composition().is_none();
        if settled {
            self.composing = false;
        }
        settled
    }

    pub fn place_composition(&self, scene: &Scene, window: HWND) {
        let (Some(origin), Some(area)) = (scene.origin_of(window), self.composition_area(scene))
        else {
            return;
        };
        let (x, y) = (origin.x as f32, origin.y as f32);
        let local = RECT {
            left: (area.left - x).floor() as i32,
            top: (area.top - y).floor() as i32,
            right: (area.right - x).ceil() as i32,
            bottom: (area.bottom - y).ceil() as i32,
        };
        ime::place(window, local);
    }

    pub(super) fn shown_marks(&self) -> Cow<'_, [Mark]> {
        let marks = self.markup.marks();
        let Some(composed) = self.markup.composed() else {
            return Cow::Borrowed(marks);
        };
        let shown = marks.iter().map(|mark| {
            if mark.id == composed.mark.id {
                composed.mark.clone()
            } else {
                mark.clone()
            }
        });
        Cow::Owned(shown.collect())
    }

    fn composition_area(&self, scene: &Scene) -> Option<D2D_RECT_F> {
        let (mark, caret, target) = match self.markup.composed() {
            Some(composed) => {
                let target = composed
                    .clauses
                    .iter()
                    .find(|(_, clause)| *clause == Clause::Target)
                    .map(|(range, _)| range.clone());
                (composed.mark, composed.caret, target)
            }
            None => (
                self.markup.editing()?.clone(),
                self.markup.cursor()?.caret,
                None,
            ),
        };
        let gpu = scene.gpu_near(Point::default()).ok()?;
        let typed = Typed::of(&gpu.text, &mark)?;
        target
            .and_then(|range| typed.selection(range).into_iter().next())
            .or_else(|| typed.caret(caret))
    }
}
