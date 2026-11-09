use quantette::PaletteBuf;
use quantette::color_map::{IndexedColorMap, NearestNeighborColorMap};
use quantette::deps::palette::Srgb;

use super::change::Rgb;

type Channels = [f32; 3];

pub(super) fn dither(
    pixels: &[Rgb],
    changed: &[bool],
    width: usize,
    colours: &[Rgb],
    transparent: u8,
) -> Option<Vec<u8>> {
    if width == 0 {
        return None;
    }
    let palette = PaletteBuf::new(
        colours
            .iter()
            .map(|colour| {
                let [red, green, blue] = channels(*colour);
                Srgb::new(red, green, blue)
            })
            .collect(),
    )
    .ok()?;
    let nearest: NearestNeighborColorMap<Srgb<f32>, f32, 3> = NearestNeighborColorMap::new(palette);
    let mut indices = vec![transparent; pixels.len()];
    let mut current = vec![[0.0; 3]; width + 2];
    let mut next = vec![[0.0; 3]; width + 2];
    for (y, ((row, row_changed), row_indices)) in pixels
        .chunks_exact(width)
        .zip(changed.chunks_exact(width))
        .zip(indices.chunks_exact_mut(width))
        .enumerate()
    {
        let forward = y % 2 == 0;
        for step in 0..width {
            let x = if forward { step } else { width - 1 - step };
            let (Some(pixel), Some(true), Some(index)) = (
                row.get(x),
                row_changed.get(x).copied(),
                row_indices.get_mut(x),
            ) else {
                continue;
            };
            let slot = x + 1;
            let carried = current.get(slot).copied().unwrap_or_default();
            let [red, green, blue] = sum(channels(*pixel), carried).map(|c| c.clamp(0.0, 255.0));
            let chosen = nearest.palette_index(&Srgb::new(red, green, blue));
            *index = chosen;
            let Some(colour) = colours.get(usize::from(chosen)) else {
                continue;
            };
            let error = difference([red, green, blue], channels(*colour));
            let (ahead, behind) = if forward {
                (slot + 1, slot - 1)
            } else {
                (slot - 1, slot + 1)
            };
            spread(&mut current, ahead, error, 7.0 / 16.0);
            spread(&mut next, behind, error, 3.0 / 16.0);
            spread(&mut next, slot, error, 5.0 / 16.0);
            spread(&mut next, ahead, error, 1.0 / 16.0);
        }
        std::mem::swap(&mut current, &mut next);
        next.fill([0.0; 3]);
    }
    Some(indices)
}

fn channels([red, green, blue]: Rgb) -> Channels {
    [f32::from(red), f32::from(green), f32::from(blue)]
}

fn sum([a, b, c]: Channels, [x, y, z]: Channels) -> Channels {
    [a + x, b + y, c + z]
}

fn difference([a, b, c]: Channels, [x, y, z]: Channels) -> Channels {
    [a - x, b - y, c - z]
}

fn spread(errors: &mut [Channels], at: usize, [red, green, blue]: Channels, share: f32) {
    if let Some([r, g, b]) = errors.get_mut(at) {
        *r += red * share;
        *g += green * share;
        *b += blue * share;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = [0, 0, 0];
    const WHITE: Rgb = [255, 255, 255];

    #[test]
    fn mid_grey_between_black_and_white_comes_out_half_white() {
        let pixels = vec![[128, 128, 128]; 32 * 32];
        let changed = vec![true; pixels.len()];
        let indices = dither(&pixels, &changed, 32, &[BLACK, WHITE], 2).unwrap();
        let white = indices.iter().filter(|index| **index == 1).count();
        assert!(indices.iter().all(|index| *index < 2));
        assert!((480..=560).contains(&white), "{white} of 1024 white");
    }

    #[test]
    fn a_colour_in_the_palette_is_used_as_is() {
        let red = [200, 10, 10];
        let pixels = vec![red; 16];
        let changed = vec![true; 16];
        let indices = dither(&pixels, &changed, 4, &[BLACK, red, WHITE], 3).unwrap();
        assert!(indices.iter().all(|index| *index == 1));
    }

    #[test]
    fn unchanged_pixels_get_the_transparent_index() {
        let pixels = vec![[90, 90, 90]; 9];
        let changed = [true, false, true, false, false, false, true, false, true];
        let indices = dither(&pixels, &changed, 3, &[BLACK, WHITE], 2).unwrap();
        for (index, changed) in indices.iter().zip(changed) {
            assert_eq!(*index == 2, !changed);
        }
    }

    #[test]
    fn nothing_is_made_without_a_width_or_a_palette() {
        assert!(dither(&[BLACK], &[true], 0, &[BLACK], 1).is_none());
        assert!(dither(&[BLACK], &[true], 1, &[], 0).is_none());
    }
}
