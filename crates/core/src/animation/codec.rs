use std::iter;

use super::change::Cut;

type Rgba = [u8; 4];

const OP_INDEX: u8 = 0x00;
const OP_DIFF: u8 = 0x40;
const OP_LUMA: u8 = 0x80;
const OP_RUN: u8 = 0xc0;
const OP_RGB: u8 = 0xfe;
const OP_RGBA: u8 = 0xff;
const TAG: u8 = 0xc0;
const LONGEST_RUN: u8 = 62;

const UNCHANGED: Rgba = [0, 0, 0, 0];
const START: Rgba = [0, 0, 0, 255];

// QOI's ops without its header and end marker; an unchanged pixel is transparent black.
pub(super) fn compress(cut: &Cut, out: &mut Vec<u8>) {
    out.clear();
    let mut index = [UNCHANGED; 64];
    let mut previous = START;
    let mut run = 0u8;
    for (pixel, changed) in cut.pixels.iter().zip(&cut.changed) {
        let current = if *changed {
            let [red, green, blue] = *pixel;
            [red, green, blue, u8::MAX]
        } else {
            UNCHANGED
        };
        if current == previous {
            run += 1;
            if run == LONGEST_RUN {
                out.push(OP_RUN | (run - 1));
                run = 0;
            }
            continue;
        }
        if run > 0 {
            out.push(OP_RUN | (run - 1));
            run = 0;
        }
        let slot = hash(current);
        match index.get_mut(slot) {
            Some(entry) if *entry == current => out.push(OP_INDEX | slot as u8),
            entry => {
                if let Some(entry) = entry {
                    *entry = current;
                }
                write_step(previous, current, out);
            }
        }
        previous = current;
    }
    if run > 0 {
        out.push(OP_RUN | (run - 1));
    }
}

pub(super) fn decompress(data: &[u8], pixel_count: usize) -> Option<Cut> {
    let mut cut = Cut {
        pixels: Vec::with_capacity(pixel_count),
        changed: Vec::with_capacity(pixel_count),
    };
    let mut index = [UNCHANGED; 64];
    let mut previous = START;
    let mut bytes = data.iter().copied();
    while cut.pixels.len() < pixel_count {
        let tag = bytes.next()?;
        let [red, green, blue, alpha] = previous;
        let repeat = if tag == OP_RGB {
            previous = [bytes.next()?, bytes.next()?, bytes.next()?, alpha];
            1
        } else if tag == OP_RGBA {
            previous = [bytes.next()?, bytes.next()?, bytes.next()?, bytes.next()?];
            let [.., alpha] = previous;
            if alpha != 0 && alpha != u8::MAX {
                return None;
            }
            1
        } else {
            match tag & TAG {
                OP_INDEX => {
                    previous = *index.get(usize::from(tag))?;
                    1
                }
                OP_DIFF => {
                    previous = [
                        red.wrapping_add((tag >> 4) & 0x03).wrapping_sub(2),
                        green.wrapping_add((tag >> 2) & 0x03).wrapping_sub(2),
                        blue.wrapping_add(tag & 0x03).wrapping_sub(2),
                        alpha,
                    ];
                    1
                }
                OP_LUMA => {
                    let second = bytes.next()?;
                    let green_step = (tag & 0x3f).wrapping_sub(32);
                    previous = [
                        red.wrapping_add(green_step)
                            .wrapping_sub(8)
                            .wrapping_add(second >> 4),
                        green.wrapping_add(green_step),
                        blue.wrapping_add(green_step)
                            .wrapping_sub(8)
                            .wrapping_add(second & 0x0f),
                        alpha,
                    ];
                    1
                }
                _ => usize::from(tag & 0x3f) + 1,
            }
        };
        if !(OP_RUN..OP_RGB).contains(&tag)
            && let Some(entry) = index.get_mut(hash(previous))
        {
            *entry = previous;
        }
        if repeat > pixel_count - cut.pixels.len() {
            return None;
        }
        let [red, green, blue, alpha] = previous;
        cut.pixels
            .extend(iter::repeat_n([red, green, blue], repeat));
        cut.changed.extend(iter::repeat_n(alpha != 0, repeat));
    }
    bytes.next().is_none().then_some(cut)
}

fn write_step(previous: Rgba, current: Rgba, out: &mut Vec<u8>) {
    let [red, green, blue, alpha] = current;
    let [previous_red, previous_green, previous_blue, previous_alpha] = previous;
    if alpha != previous_alpha {
        out.extend_from_slice(&[OP_RGBA, red, green, blue, alpha]);
        return;
    }
    let red_step = red.wrapping_sub(previous_red) as i8;
    let green_step = green.wrapping_sub(previous_green) as i8;
    let blue_step = blue.wrapping_sub(previous_blue) as i8;
    let red_from_green = red_step.wrapping_sub(green_step);
    let blue_from_green = blue_step.wrapping_sub(green_step);
    let small = -2..=1;
    if small.contains(&red_step) && small.contains(&green_step) && small.contains(&blue_step) {
        out.push(
            OP_DIFF
                | (((red_step + 2) as u8) << 4)
                | (((green_step + 2) as u8) << 2)
                | ((blue_step + 2) as u8),
        );
    } else if (-32..=31).contains(&green_step)
        && (-8..=7).contains(&red_from_green)
        && (-8..=7).contains(&blue_from_green)
    {
        out.push(OP_LUMA | ((green_step + 32) as u8));
        out.push((((red_from_green + 8) as u8) << 4) | ((blue_from_green + 8) as u8));
    } else {
        out.extend_from_slice(&[OP_RGB, red, green, blue]);
    }
}

fn hash([red, green, blue, alpha]: Rgba) -> usize {
    (usize::from(red) * 3
        + usize::from(green) * 5
        + usize::from(blue) * 7
        + usize::from(alpha) * 11)
        % 64
}

#[cfg(test)]
mod tests {
    use super::super::change::Rgb;
    use super::*;

    fn cut(pixels: &[Option<Rgb>]) -> Cut {
        Cut {
            pixels: pixels
                .iter()
                .map(|pixel| pixel.unwrap_or([0, 0, 0]))
                .collect(),
            changed: pixels.iter().map(Option::is_some).collect(),
        }
    }

    fn round_trip(pixels: &[Option<Rgb>]) -> Vec<u8> {
        let original = cut(pixels);
        let mut data = Vec::new();
        compress(&original, &mut data);
        let back = decompress(&data, pixels.len()).unwrap();
        assert_eq!(back.changed, original.changed);
        assert_eq!(back.pixels, original.pixels);
        data
    }

    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn byte(&mut self) -> u8 {
            (self.next() >> 24) as u8
        }
    }

    #[test]
    fn each_step_is_written_the_way_qoi_writes_it() {
        let data = round_trip(&[
            Some([100, 100, 100]),
            Some([101, 99, 100]),
            Some([111, 104, 99]),
            None,
            None,
            Some([101, 99, 100]),
        ]);
        assert_eq!(
            data,
            [0xfe, 100, 100, 100, 0x76, 0xa5, 0xd2, 0x00, 0xc0, 0x0f]
        );
    }

    #[test]
    fn steps_wrap_around_like_bytes() {
        let data = round_trip(&[Some([255, 255, 255]), Some([0, 0, 0])]);
        assert_eq!(data, [0x55, 0x7f]);
    }

    #[test]
    fn a_long_run_is_split_below_the_eight_bit_tags() {
        let pixels = vec![Some([10, 20, 30]); 200];
        let data = round_trip(&pixels);
        assert_eq!(data, [0xfe, 10, 20, 30, 0xfd, 0xfd, 0xfd, 0xcc]);
    }

    #[test]
    fn unchanged_pixels_cost_almost_nothing() {
        let data = round_trip(&[None; 1000]);
        assert_eq!(data.len(), 18);
    }

    #[test]
    fn the_colour_behind_an_unchanged_pixel_is_not_kept() {
        let mut hidden = cut(&[Some([1, 2, 3]), None, Some([200, 0, 0])]);
        let mut data = vec![9; 40];
        compress(&hidden, &mut data);
        hidden.pixels[1] = [77, 88, 99];
        let mut other = Vec::new();
        compress(&hidden, &mut other);
        assert_eq!(data, other);
        assert_eq!(decompress(&data, 3).unwrap().pixels[1], [0, 0, 0]);
    }

    #[test]
    fn nothing_compresses_to_nothing() {
        let mut data = vec![1, 2, 3];
        compress(&cut(&[]), &mut data);
        assert!(data.is_empty());
        let back = decompress(&data, 0).unwrap();
        assert!(back.pixels.is_empty() && back.changed.is_empty());
    }

    #[test]
    fn damaged_data_is_refused() {
        let mut data = Vec::new();
        compress(&cut(&[Some([100, 0, 0]), None, Some([0, 0, 0])]), &mut data);
        assert!(decompress(&data, 3).is_some());
        assert!(decompress(&data[..data.len() - 1], 3).is_none());
        assert!(decompress(&[data.as_slice(), &[0x00]].concat(), 3).is_none());
        assert!(decompress(&data, 4).is_none());
        assert!(decompress(&[0xc9], 5).is_none());
        assert!(decompress(&[0xff, 1, 2, 3, 7], 1).is_none());
        assert!(decompress(&[], 1).is_none());
    }

    #[test]
    fn mixed_pixels_come_back_unchanged() {
        for seed in 1..=20u64 {
            let mut noise = Noise(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut last = [0u8; 3];
            let pixels: Vec<Option<Rgb>> = (0..5_000)
                .map(|_| {
                    let pixel = match noise.next() % 6 {
                        0 => return None,
                        1 => last,
                        2 => last
                            .map(|channel| channel.wrapping_add(noise.byte() % 4).wrapping_sub(2)),
                        3 => last.map(|channel| {
                            channel.wrapping_add(noise.byte() % 40).wrapping_sub(20)
                        }),
                        _ => [noise.byte(), noise.byte(), noise.byte()],
                    };
                    last = pixel;
                    Some(pixel)
                })
                .collect();
            round_trip(&pixels);
        }
    }
}
