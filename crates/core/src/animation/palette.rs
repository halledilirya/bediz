use std::collections::HashMap;

use quantette::PaletteSize;
use quantette::deps::palette::{Srgb, cast};
use quantette::wu::{BinnerU8x3, WuU8x3};

use super::change::Rgb;

pub(super) const MAX_COLOURS: u8 = 255;

pub(super) enum Palette {
    Exact {
        colours: Vec<Rgb>,
        indices: HashMap<Rgb, u8>,
    },
    Reduced {
        colours: Vec<Rgb>,
    },
}

impl Palette {
    pub(super) fn colours(&self) -> &[Rgb] {
        match self {
            Self::Exact { colours, .. } | Self::Reduced { colours } => colours,
        }
    }
}

pub(super) fn build(colours: &[Rgb]) -> Option<Palette> {
    exact(colours).or_else(|| reduced(colours))
}

fn exact(colours: &[Rgb]) -> Option<Palette> {
    let mut indices = HashMap::new();
    let mut unique = Vec::new();
    let mut last = None;
    for colour in colours {
        if last == Some(*colour) {
            continue;
        }
        last = Some(*colour);
        if indices.contains_key(colour) {
            continue;
        }
        let index = u8::try_from(unique.len())
            .ok()
            .filter(|index| *index < MAX_COLOURS)?;
        indices.insert(*colour, index);
        unique.push(*colour);
    }
    (!unique.is_empty()).then_some(Palette::Exact {
        colours: unique,
        indices,
    })
}

fn reduced(colours: &[Rgb]) -> Option<Palette> {
    let srgb: &[Srgb<u8>] = cast::from_array_slice(colours);
    let wu = WuU8x3::run_slice(srgb, BinnerU8x3::rgb()).ok()?;
    let palette = wu.palette(PaletteSize::from_u8_clamped(MAX_COLOURS));
    Some(Palette::Reduced {
        colours: cast::into_array_slice(palette.as_slice()).to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distinct(count: usize) -> Vec<Rgb> {
        (0..count)
            .map(|n| [(n % 256) as u8, (n / 256) as u8, 7])
            .collect()
    }

    #[test]
    fn few_colours_are_kept_exactly_in_the_order_they_appear() {
        let red = [255, 0, 0];
        let blue = [0, 0, 255];
        let Some(Palette::Exact { colours, indices }) = build(&[red, red, blue, red, blue]) else {
            panic!("expected an exact palette");
        };
        assert_eq!(colours, [red, blue]);
        assert_eq!(indices.get(&red), Some(&0));
        assert_eq!(indices.get(&blue), Some(&1));
    }

    #[test]
    fn two_hundred_and_fifty_five_colours_are_still_exact() {
        let palette = build(&distinct(255)).unwrap();
        assert!(matches!(palette, Palette::Exact { .. }));
        assert_eq!(palette.colours().len(), 255);
    }

    #[test]
    fn more_colours_are_reduced_to_two_hundred_and_fifty_five() {
        let palette = build(&distinct(4096)).unwrap();
        assert!(matches!(palette, Palette::Reduced { .. }));
        assert!(!palette.colours().is_empty());
        assert!(palette.colours().len() <= usize::from(MAX_COLOURS));
    }

    #[test]
    fn no_colours_make_no_palette() {
        assert!(build(&[]).is_none());
    }
}
