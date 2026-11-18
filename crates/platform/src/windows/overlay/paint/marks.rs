//! Draws the marks on an overlay in the order they were drawn, the frame and handles of the
//! selected one, and the caret, selection and input method's composition of the text being typed.

use bediz_core::geometry::Rect;
use bediz_core::markup::composition::{Clause, Composed};
use bediz_core::markup::typing::Cursor;
use bediz_core::markup::{Colour, Mark, Shape, effect, shape};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, ID2D1Bitmap1, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Gdi::{COLOR_HIGHLIGHT, GetSysColor};
use windows_numerics::Vector2;

use super::{HANDLE_SIZE, Painter, rect_f};
use crate::windows::marking::layout::Typed;
use crate::windows::marking::paint::Ink;
use crate::windows::marking::{Decor, Preview};

const SELECTION_OPACITY: f32 = 0.4;
const UNDERLINE: f32 = 16.0;

impl Painter<'_> {
    pub fn marks(&self, decor: &Decor, area: Rect, previews: &mut Vec<(u64, ID2D1Bitmap1)>) {
        let canvas = &self.gpu.canvas;
        let ink = self.ink();
        let composed = decor.composed();
        unsafe { canvas.PushAxisAlignedClip(&rect_f(area), D2D1_ANTIALIAS_MODE_ALIASED) };
        for mark in decor.marks {
            let mark = composed
                .as_ref()
                .filter(|composed| composed.mark.id == mark.id)
                .map_or(mark, |composed| &composed.mark);
            match mark.shape {
                Shape::Redact(..) => {
                    if let Some(covered) = effect::area_of(mark) {
                        let _ = ink.cover(covered);
                    }
                }
                Shape::Blur(..) | Shape::Pixelate(..) => match decor.preview_of(mark) {
                    Some(preview) => self.preview(preview, previews, &ink),
                    None => self.placeholder(mark, &ink),
                },
                _ => {
                    let _ = ink.mark(mark);
                }
            }
        }
        unsafe { canvas.PopAxisAlignedClip() };
        previews.retain(|(stamp, _)| decor.previews.iter().any(|preview| preview.stamp == *stamp));
    }

    pub fn mark_handles(&self, mark: &Mark) {
        let gpu = self.gpu;
        let framed = matches!(
            mark.shape,
            Shape::Pen(_) | Shape::Highlighter(_) | Shape::Text { .. } | Shape::Step { .. }
        );
        if framed {
            let bounds = shape::bounds(mark).translated(-self.origin.x, -self.origin.y);
            let edge = D2D_RECT_F {
                left: bounds.left as f32 + 0.5,
                top: bounds.top as f32 + 0.5,
                right: bounds.right as f32 - 0.5,
                bottom: bounds.bottom as f32 - 0.5,
            };
            unsafe {
                gpu.canvas
                    .DrawRectangle(&edge, &gpu.outline, 1.0, None::<&ID2D1StrokeStyle>);
                gpu.canvas
                    .DrawRectangle(&edge, &gpu.border, 1.0, &gpu.dashed);
            }
        }
        let half = (HANDLE_SIZE * self.scale).round() / 2.0;
        for (_, at) in shape::corners(mark) {
            let (x, y) = (
                (at.x - self.origin.x) as f32 + 0.5,
                (at.y - self.origin.y) as f32 + 0.5,
            );
            let handle = D2D_RECT_F {
                left: x - half,
                top: y - half,
                right: x + half,
                bottom: y + half,
            };
            unsafe {
                gpu.canvas.FillRectangle(&handle, &gpu.border);
                gpu.canvas
                    .DrawRectangle(&handle, &gpu.outline, 1.0, None::<&ID2D1StrokeStyle>);
            }
        }
    }

    pub fn typing(&self, decor: &Decor) {
        match (decor.composed(), decor.editing) {
            (Some(composed), _) => self.composing(&composed),
            (None, Some(editing)) => self.caret_and_selection(editing, decor.cursor),
            (None, None) => {}
        }
    }

    fn composing(&self, composed: &Composed) {
        let mark = &composed.mark;
        let (Some(typed), Ok(brush)) = (
            Typed::of(&self.gpu.text, mark),
            self.ink().brush(mark.colour, 1.0),
        ) else {
            return;
        };
        let thin = (shape::text_size(mark.width) / UNDERLINE).round().max(1.0);
        for (range, clause) in &composed.clauses {
            let thickness = if *clause == Clause::Target {
                thin * 2.0
            } else {
                thin
            };
            for part in typed.selection(range.clone()) {
                let part = self.local(part);
                let y = part.bottom - thickness / 2.0;
                let from = Vector2::new(part.left + thin, y);
                let to = Vector2::new((part.right - thin).max(part.left + thin), y);
                let canvas = &self.gpu.canvas;
                unsafe {
                    match clause {
                        Clause::Unconverted => {
                            canvas.DrawLine(from, to, &brush, thickness, &self.gpu.dashed);
                        }
                        Clause::Converted | Clause::Target => {
                            canvas.DrawLine(from, to, &brush, thickness, None::<&ID2D1StrokeStyle>);
                        }
                    }
                }
            }
        }
        if let Some(caret) = typed.caret(composed.caret) {
            unsafe { self.gpu.canvas.FillRectangle(&self.local(caret), &brush) };
        }
    }

    fn caret_and_selection(&self, mark: &Mark, cursor: Cursor) {
        let Some(typed) = Typed::of(&self.gpu.text, mark) else {
            return;
        };
        let ink = self.ink();
        let [red, green, blue, _] = unsafe { GetSysColor(COLOR_HIGHLIGHT) }.to_le_bytes();
        if let Ok(highlight) = ink.brush(Colour::new(red, green, blue), SELECTION_OPACITY) {
            for part in typed.selection(cursor.range()) {
                unsafe { self.gpu.canvas.FillRectangle(&self.local(part), &highlight) };
            }
        }
        if let (Some(caret), Ok(brush)) = (typed.caret(cursor.caret), ink.brush(mark.colour, 1.0)) {
            unsafe { self.gpu.canvas.FillRectangle(&self.local(caret), &brush) };
        }
    }

    fn local(&self, area: D2D_RECT_F) -> D2D_RECT_F {
        let (x, y) = (self.origin.x as f32, self.origin.y as f32);
        D2D_RECT_F {
            left: area.left - x,
            top: area.top - y,
            right: area.right - x,
            bottom: area.bottom - y,
        }
    }

    fn preview(&self, preview: &Preview, previews: &mut Vec<(u64, ID2D1Bitmap1)>, ink: &Ink) {
        let found = previews
            .iter()
            .find(|(stamp, _)| *stamp == preview.stamp)
            .map(|(_, bitmap)| bitmap.clone());
        let bitmap = match found {
            Some(bitmap) => bitmap,
            None => match self.gpu.memory_bitmap(&preview.effect.image) {
                Ok(bitmap) => {
                    previews.push((preview.stamp, bitmap.clone()));
                    bitmap
                }
                Err(_) => return,
            },
        };
        ink.picture(&bitmap, preview.effect.area, false);
    }

    fn placeholder(&self, mark: &Mark, ink: &Ink) {
        let (Shape::Blur(from, to) | Shape::Pixelate(from, to)) = mark.shape else {
            return;
        };
        let area = ink.boxed(from, to);
        let gpu = self.gpu;
        unsafe {
            gpu.canvas
                .DrawRectangle(&area, &gpu.outline, 1.0, None::<&ID2D1StrokeStyle>);
            gpu.canvas
                .DrawRectangle(&area, &gpu.border, 1.0, &gpu.dashed);
        }
    }

    fn ink(&self) -> Ink<'_> {
        Ink {
            canvas: &self.gpu.canvas,
            text: &self.gpu.text,
            dx: self.origin.x as f32,
            dy: self.origin.y as f32,
        }
    }
}
