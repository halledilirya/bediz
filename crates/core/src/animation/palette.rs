//! Builds a frame's palette: every colour exactly when there are 255 or fewer, otherwise the
//! colours covering large areas exactly and a reduced set for the rest.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use quantette::PaletteSize;
use quantette::deps::palette::{Srgb, cast};
use quantette::wu::{BinnerU8x3, WuU8x3};

use super::change::Rgb;

pub(super) const MAX_COLOURS: u8 = 255;
const POPULAR_SHARE: usize = 512;
const MOST_POPULAR: usize = 128;
const SAMPLES: usize = 65_536;
const MARGIN: usize = 4;
const SEED: u64 = 0x9e37_79b9_7f4a_7c15;

pub(super) type ColourMap<V> = HashMap<Rgb, V, BuildHasherDefault<ColourHasher>>;

// A key is three bytes of a frame, so the default hasher's guard against crafted keys only costs time.
#[derive(Default)]
pub(super) struct ColourHasher(u64);

impl Hasher for ColourHasher {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 << 8) | u64::from(*byte);
        }
    }

    fn finish(&self) -> u64 {
        let mixed = self.0.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        mixed ^ (mixed >> 29)
    }
}

pub(super) enum Palette {
    Exact {
        colours: Vec<Rgb>,
        indices: ColourMap<u8>,
    },
    Reduced {
        colours: Vec<Rgb>,
        kept: ColourMap<u8>,
    },
}

impl Palette {
    pub(super) fn colours(&self) -> &[Rgb] {
        match self {
            Self::Exact { colours, .. } | Self::Reduced { colours, .. } => colours,
        }
    }
}

pub(super) fn build(colours: &[Rgb]) -> Option<Palette> {
    exact(colours).or_else(|| reduced(colours))
}

fn exact(colours: &[Rgb]) -> Option<Palette> {
    let mut indices = ColourMap::default();
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

// Colours covering a large area go in exactly, since Wu's box averages would dither flat areas into noise.
fn reduced(colours: &[Rgb]) -> Option<Palette> {
    let mut palette = popular(colours);
    let kept: ColourMap<u8> = palette
        .iter()
        .zip(0u8..)
        .map(|(colour, index)| (*colour, index))
        .collect();
    let rest: Vec<Rgb> = colours
        .iter()
        .filter(|colour| !kept.contains_key(*colour))
        .copied()
        .collect();
    let room = MAX_COLOURS.saturating_sub(u8::try_from(palette.len()).ok()?);
    if !rest.is_empty() && room > 0 {
        palette.extend(wu(&rest, room)?);
    }
    (!palette.is_empty()).then_some(Palette::Reduced {
        colours: palette,
        kept,
    })
}

fn popular(colours: &[Rgb]) -> Vec<Rgb> {
    let seen = sample(colours);
    let floor = (colours.len().min(SAMPLES) / POPULAR_SHARE / MARGIN).max(1);
    let mut counts: ColourMap<usize> = seen
        .into_iter()
        .filter(|(_, hits)| *hits >= floor)
        .map(|(colour, _)| (colour, 0))
        .collect();
    if counts.is_empty() {
        return Vec::new();
    }
    for run in colours.chunk_by(|a, b| a == b) {
        if let Some(first) = run.first()
            && let Some(count) = counts.get_mut(first)
        {
            *count += run.len();
        }
    }
    let least = (colours.len() / POPULAR_SHARE).max(1);
    let mut popular: Vec<(Rgb, usize)> = counts
        .into_iter()
        .filter(|(_, count)| *count >= least)
        .collect();
    popular.sort_by(|(a, first), (b, second)| second.cmp(first).then(a.cmp(b)));
    popular.truncate(MOST_POPULAR);
    popular.into_iter().map(|(colour, _)| colour).collect()
}

// A frame of millions of distinct colours would grow a table as large, so candidates come from a bounded sample.
fn sample(colours: &[Rgb]) -> ColourMap<usize> {
    let mut seen: ColourMap<usize> = ColourMap::default();
    if colours.len() <= SAMPLES {
        for run in colours.chunk_by(|a, b| a == b) {
            if let Some(colour) = run.first() {
                *seen.entry(*colour).or_default() += run.len();
            }
        }
        return seen;
    }
    let length = u64::try_from(colours.len()).unwrap_or(u64::MAX);
    let mut state = SEED;
    for _ in 0..SAMPLES {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let index = usize::try_from(state % length).unwrap_or(0);
        if let Some(colour) = colours.get(index) {
            *seen.entry(*colour).or_default() += 1;
        }
    }
    seen
}

fn wu(colours: &[Rgb], size: u8) -> Option<Vec<Rgb>> {
    let srgb: &[Srgb<u8>] = cast::from_array_slice(colours);
    let wu = WuU8x3::run_slice(srgb, BinnerU8x3::rgb()).ok()?;
    let palette = wu.palette(PaletteSize::from_u8_clamped(size));
    Some(cast::into_array_slice(palette.as_slice()).to_vec())
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

    #[test]
    fn a_colour_covering_a_large_area_is_kept_exactly() {
        let background = [31, 31, 31];
        let mut colours = vec![background; 20_000];
        colours.extend(distinct(4096));
        let Some(Palette::Reduced {
            colours: palette,
            kept,
        }) = build(&colours)
        else {
            panic!("expected a reduced palette");
        };
        assert_eq!(kept.len(), 1);
        let index = usize::from(*kept.get(&background).unwrap());
        assert_eq!(palette[index], background);
        assert!(palette.len() <= usize::from(MAX_COLOURS));
    }

    #[test]
    fn every_colour_is_its_own_key() {
        let mut map = ColourMap::default();
        for (n, colour) in distinct(4096).into_iter().enumerate() {
            map.insert(colour, n);
        }
        map.insert([255, 255, 255], 4096);
        assert_eq!(map.len(), 4097);
        assert_eq!(map.get(&[5, 3, 7]), Some(&(3 * 256 + 5)));
        assert_eq!(map.get(&[255, 255, 255]), Some(&4096));
        assert_eq!(map.get(&[5, 3, 8]), None);
    }

    fn millions(count: usize) -> Vec<Rgb> {
        (0..count)
            .map(|n| {
                [
                    (n & 255) as u8,
                    ((n >> 8) & 255) as u8,
                    ((n >> 16) & 255) as u8,
                ]
            })
            .collect()
    }

    #[test]
    fn the_counting_stays_small_however_many_colours_a_frame_holds() {
        let colours = millions(1920 * 1080);
        assert!(sample(&colours).len() <= SAMPLES);
        assert!(popular(&colours).is_empty());
    }

    #[test]
    fn a_background_among_millions_of_colours_is_kept() {
        let background = [10, 10, 200];
        let mut colours = millions(1920 * 1080);
        for colour in colours.iter_mut().step_by(3) {
            *colour = background;
        }
        assert_eq!(popular(&colours), [background]);
    }

    #[test]
    fn two_colours_in_a_fine_pattern_are_both_kept_in_a_fixed_order() {
        let (first, second) = ([1, 2, 250], [3, 4, 251]);
        let mut colours: Vec<Rgb> = (0..200_000)
            .map(|n| if n % 2 == 0 { second } else { first })
            .collect();
        colours.extend(millions(100_000));
        assert_eq!(popular(&colours), [first, second]);
    }

    #[test]
    fn colours_spread_thin_leave_the_palette_to_wu() {
        let Some(Palette::Reduced { kept, .. }) = build(&distinct(4096)) else {
            panic!("expected a reduced palette");
        };
        assert!(kept.is_empty());
    }
}
