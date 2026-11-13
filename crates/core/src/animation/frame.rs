//! Turns a frame's changed area into a GIF frame with its own colour table and transparent
//! unchanged pixels, and makes the invisible frame that carries the rest of an overlong delay.

use std::borrow::Cow;

use gif::{DisposalMethod, Frame};

use super::change::{Area, Cut, Rgb};
use super::dither::dither;
use super::palette::{self, Palette};

pub(super) struct IndexedFrame {
    area: Area,
    table: Vec<u8>,
    transparent: Option<u8>,
    indices: Vec<u8>,
}

impl IndexedFrame {
    pub(super) fn new(cut: &Cut, area: Area) -> Option<Self> {
        let changed: Vec<Rgb> = cut
            .pixels
            .iter()
            .zip(&cut.changed)
            .filter(|(_, changed)| **changed)
            .map(|(pixel, _)| *pixel)
            .collect();
        let palette = palette::build(&changed)?;
        let colours = palette.colours();
        let unused = u8::try_from(colours.len()).ok()?;
        let transparent = cut.changed.iter().any(|changed| !changed).then_some(unused);
        let indices = match &palette {
            Palette::Exact { indices, .. } => cut
                .pixels
                .iter()
                .zip(&cut.changed)
                .map(|(pixel, changed)| {
                    if *changed {
                        indices.get(pixel).copied().unwrap_or(unused)
                    } else {
                        unused
                    }
                })
                .collect(),
            Palette::Reduced { colours, kept } => dither(
                &cut.pixels,
                &cut.changed,
                usize::from(area.width),
                colours,
                unused,
                kept,
            )?,
        };
        let mut table: Vec<u8> = colours.iter().flatten().copied().collect();
        if transparent.is_some() {
            table.extend_from_slice(&[0, 0, 0]);
        }
        Some(Self {
            area,
            table,
            transparent,
            indices,
        })
    }

    pub(super) fn into_gif(self, delay: u16) -> Frame<'static> {
        Frame {
            delay,
            dispose: DisposalMethod::Keep,
            transparent: self.transparent,
            needs_user_input: false,
            top: self.area.top,
            left: self.area.left,
            width: self.area.width,
            height: self.area.height,
            interlaced: false,
            palette: Some(self.table),
            buffer: Cow::Owned(self.indices),
        }
    }
}

pub(super) fn still(delay: u16) -> Frame<'static> {
    Frame {
        delay,
        dispose: DisposalMethod::Keep,
        transparent: Some(0),
        needs_user_input: false,
        top: 0,
        left: 0,
        width: 1,
        height: 1,
        interlaced: false,
        palette: Some(vec![0, 0, 0]),
        buffer: Cow::Owned(vec![0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = [255, 0, 0];
    const GREY: Rgb = [90, 90, 90];

    fn area(width: u16, height: u16) -> Area {
        Area {
            left: 1,
            top: 2,
            width,
            height,
        }
    }

    #[test]
    fn a_fully_changed_frame_has_no_transparent_index() {
        let cut = Cut {
            pixels: vec![RED, GREY],
            changed: vec![true, true],
        };
        let frame = IndexedFrame::new(&cut, area(2, 1)).unwrap().into_gif(7);
        assert_eq!(frame.transparent, None);
        assert_eq!(frame.palette, Some(vec![255, 0, 0, 90, 90, 90]));
        assert_eq!(frame.buffer.as_ref(), [0, 1]);
        assert_eq!(
            (frame.left, frame.top, frame.width, frame.height),
            (1, 2, 2, 1)
        );
        assert_eq!(frame.delay, 7);
        assert_eq!(frame.dispose, DisposalMethod::Keep);
    }

    #[test]
    fn unchanged_pixels_point_past_the_colours_to_a_transparent_entry() {
        let cut = Cut {
            pixels: vec![RED, GREY, RED],
            changed: vec![true, false, true],
        };
        let frame = IndexedFrame::new(&cut, area(3, 1)).unwrap().into_gif(2);
        assert_eq!(frame.transparent, Some(1));
        assert_eq!(frame.palette, Some(vec![255, 0, 0, 0, 0, 0]));
        assert_eq!(frame.buffer.as_ref(), [0, 1, 0]);
    }

    #[test]
    fn many_colours_are_dithered_into_a_full_palette() {
        let pixels: Vec<Rgb> = (0..1024u32)
            .map(|n| [(n % 32 * 8) as u8, (n / 32 * 8) as u8, 128])
            .collect();
        let mut changed = vec![true; pixels.len()];
        changed[0] = false;
        let cut = Cut { pixels, changed };
        let frame = IndexedFrame::new(&cut, area(32, 32)).unwrap().into_gif(2);
        let entries = frame.palette.as_ref().unwrap().len() / 3;
        assert!(entries <= 256);
        assert_eq!(frame.transparent, Some((entries - 1) as u8));
        assert_eq!(frame.buffer[0], (entries - 1) as u8);
        assert!(
            frame.buffer[1..]
                .iter()
                .all(|index| usize::from(*index) < entries - 1)
        );
    }
}
