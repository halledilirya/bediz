//! Lays out a text mark with DirectWrite the way it is drawn: its size, where its caret and its
//! selected part lie, and the character a point falls on.

use std::ops::Range;

use bediz_core::geometry::Point;
use bediz_core::markup::{Extent, Mark, Shape, shape};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_HIT_TEST_METRICS,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteFontCollection,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::core::{BOOL, Result, w};

const UNBOUNDED: f32 = 100_000.0;
const MEASURED_SIZE: f32 = 100.0;

pub struct Typed {
    layout: IDWriteTextLayout,
    text: String,
    size: f32,
    at: Point,
}

impl Typed {
    pub fn of(factory: &IDWriteFactory, mark: &Mark) -> Option<Self> {
        let Shape::Text { at, text, .. } = &mark.shape else {
            return None;
        };
        let size = shape::text_size(mark.width);
        Some(Self {
            layout: text_layout(factory, text, size).ok()?,
            text: text.clone(),
            size,
            at: *at,
        })
    }

    pub fn caret(&self, index: usize) -> Option<D2D_RECT_F> {
        let (mut x, mut y) = (0.0, 0.0);
        let mut metrics = DWRITE_HIT_TEST_METRICS::default();
        let position = utf16_index(&self.text, index);
        unsafe {
            self.layout
                .HitTestTextPosition(position, false, &mut x, &mut y, &mut metrics)
        }
        .ok()?;
        let thickness = (self.size / 12.0).max(1.0);
        Some(self.placed(x, metrics.top, thickness, metrics.height.max(self.size)))
    }

    pub fn selection(&self, range: Range<usize>) -> Vec<D2D_RECT_F> {
        let start = utf16_index(&self.text, range.start);
        let length = utf16_index(&self.text, range.end).saturating_sub(start);
        if length == 0 {
            return Vec::new();
        }
        let mut count = 0u32;
        let _ = unsafe {
            self.layout
                .HitTestTextRange(start, length, 0.0, 0.0, None, &mut count)
        };
        let mut metrics = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
        let found = unsafe {
            self.layout
                .HitTestTextRange(start, length, 0.0, 0.0, Some(&mut metrics), &mut count)
        };
        if found.is_err() {
            return Vec::new();
        }
        metrics
            .iter()
            .take(count as usize)
            .map(|part| self.placed(part.left, part.top, part.width, part.height))
            .collect()
    }

    pub fn index_at(&self, point: Point) -> Option<usize> {
        let x = (point.x - self.at.x) as f32 + 0.5;
        let y = (point.y - self.at.y) as f32 + 0.5;
        let (mut trailing, mut inside) = (BOOL(0), BOOL(0));
        let mut metrics = DWRITE_HIT_TEST_METRICS::default();
        unsafe {
            self.layout
                .HitTestPoint(x, y, &mut trailing, &mut inside, &mut metrics)
        }
        .ok()?;
        let after = if trailing.as_bool() {
            metrics.length
        } else {
            0
        };
        Some(byte_index(&self.text, metrics.textPosition + after))
    }

    fn placed(&self, x: f32, y: f32, width: f32, height: f32) -> D2D_RECT_F {
        let (left, top) = (self.at.x as f32 + x, self.at.y as f32 + y);
        D2D_RECT_F {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }
}

pub fn measure(factory: &IDWriteFactory, text: &str) -> Option<Extent> {
    if text.is_empty() {
        return None;
    }
    let layout = text_layout(factory, text, MEASURED_SIZE).ok()?;
    let mut metrics = DWRITE_TEXT_METRICS::default();
    unsafe { layout.GetMetrics(&mut metrics) }.ok()?;
    let width = metrics.widthIncludingTrailingWhitespace;
    let inked = unsafe {
        layout
            .SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)
            .and_then(|()| layout.SetMaxWidth(width))
            .and_then(|()| layout.GetOverhangMetrics())
    }
    .map_or(width, |overhang| width + overhang.right);
    Some(Extent {
        across: width.max(inked) / MEASURED_SIZE,
        down: metrics.height / MEASURED_SIZE,
    })
}

pub fn text_format(
    factory: &IDWriteFactory,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    centred: bool,
) -> Result<IDWriteTextFormat> {
    let format = unsafe {
        factory.CreateTextFormat(
            w!("Segoe UI"),
            None::<&IDWriteFontCollection>,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size.max(1.0),
            w!(""),
        )
    }?;
    if centred {
        unsafe {
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        }
    }
    Ok(format)
}

pub fn text_layout(factory: &IDWriteFactory, text: &str, size: f32) -> Result<IDWriteTextLayout> {
    let format = text_format(factory, size, DWRITE_FONT_WEIGHT_SEMI_BOLD, false)?;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let layout = unsafe { factory.CreateTextLayout(&wide, &format, UNBOUNDED, UNBOUNDED) }?;
    // The emoji font has no semibold face, and the bold DirectWrite makes up is cut at its right edge.
    for (start, length) in emoji_runs(text) {
        let range = DWRITE_TEXT_RANGE {
            startPosition: start,
            length,
        };
        unsafe { layout.SetFontWeight(DWRITE_FONT_WEIGHT_NORMAL, range) }?;
    }
    Ok(layout)
}

fn emoji_runs(text: &str) -> Vec<(u32, u32)> {
    let mut parts: Vec<(u32, u32, bool)> = Vec::new();
    let mut at = 0u32;
    for character in text.chars() {
        let length = character.len_utf16() as u32;
        if character == '\u{FE0F}'
            && let Some(last) = parts.last_mut()
        {
            last.2 = true;
        }
        parts.push((at, length, emoji_part(character)));
        at = at.saturating_add(length);
    }
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for (start, length, _) in parts.into_iter().filter(|part| part.2) {
        match runs.last_mut() {
            Some(run) if run.0 + run.1 == start => run.1 += length,
            _ => runs.push((start, length)),
        }
    }
    runs
}

fn emoji_part(character: char) -> bool {
    matches!(
        u32::from(character),
        0x1F000..=0x1FAFF
            | 0x2300..=0x23FF
            | 0x2600..=0x27BF
            | 0x2B00..=0x2BFF
            | 0x200D
            | 0x20E3
            | 0xFE0F
            | 0xE0020..=0xE007F
    )
}

fn utf16_index(text: &str, index: usize) -> u32 {
    let units = text
        .get(..index)
        .map_or(0, |before| before.encode_utf16().count());
    u32::try_from(units).unwrap_or(u32::MAX)
}

fn byte_index(text: &str, position: u32) -> usize {
    let position = position as usize;
    let mut units = 0;
    for (at, character) in text.char_indices() {
        if units >= position {
            return at;
        }
        units += character.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caret_index_counts_the_same_place_in_bytes_and_utf16() {
        let text = "aş😀b";
        assert_eq!(utf16_index(text, 7), 4);
        assert_eq!(byte_index(text, 4), 7);
        assert_eq!(byte_index(text, 3), 7);
        assert_eq!(byte_index(text, 0), 0);
        assert_eq!(byte_index(text, 99), text.len());
        assert_eq!(utf16_index(text, 3), 2);
    }

    #[test]
    fn emoji_and_their_joiners_and_selectors_are_found_in_utf16_runs() {
        assert_eq!(emoji_runs("a😀b😀😀"), [(1, 2), (4, 4)]);
        assert_eq!(emoji_runs("❤\u{FE0F}x"), [(0, 2)]);
        assert_eq!(emoji_runs("©\u{FE0F} ©"), [(0, 2)]);
        assert_eq!(emoji_runs("1\u{FE0F}\u{20E3}"), [(0, 3)]);
        assert_eq!(emoji_runs("👩\u{200D}💻!"), [(0, 5)]);
        assert!(emoji_runs("İSTANBUL, ş").is_empty());
    }
}
