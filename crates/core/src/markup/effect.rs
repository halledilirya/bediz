//! The marks made from the pixels under them, a blur and a pixelation, and the marks each one is
//! made over, since marks lie on each other in the order they were drawn.

use crate::geometry::Rect;
use crate::image::Image;

use super::{Mark, Shape, shape};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    pub id: u64,
    pub area: Rect,
    pub image: Image,
}

#[derive(Clone, Copy, Debug)]
pub struct Job<'a> {
    pub mark: &'a Mark,
    pub area: Rect,
    pub beneath: &'a [Mark],
}

pub fn area_of(mark: &Mark) -> Option<Rect> {
    match &mark.shape {
        Shape::Redact(from, to) | Shape::Blur(from, to) | Shape::Pixelate(from, to) => {
            Some(Rect::spanning(*from, *to))
        }
        _ => None,
    }
}

pub fn jobs(marks: &[Mark]) -> impl Iterator<Item = Job<'_>> {
    marks.iter().enumerate().filter_map(|(index, mark)| {
        if !matches!(mark.shape, Shape::Blur(..) | Shape::Pixelate(..)) {
            return None;
        }
        Some(Job {
            mark,
            area: area_of(mark)?,
            beneath: marks.get(..index)?,
        })
    })
}

pub fn apply(mark: &Mark, source: &Image) -> Option<Effect> {
    let area = area_of(mark)?;
    let image = match mark.shape {
        Shape::Blur(..) => blurred(source, shape::blur_radius(mark.width))?,
        Shape::Pixelate(..) => pixelated(source, shape::pixel_block(mark.width))?,
        _ => return None,
    };
    Some(Effect {
        id: mark.id,
        area,
        image,
    })
}

fn pixelated(source: &Image, block: u32) -> Option<Image> {
    let block = block.max(1) as usize;
    let (width, height) = (source.width() as usize, source.height() as usize);
    let pixels = source.bgra().as_chunks::<4>().0;
    let mut out = vec![[0u8, 0, 0, u8::MAX]; pixels.len()];
    for top in (0..height).step_by(block) {
        let bottom = (top + block).min(height);
        for left in (0..width).step_by(block) {
            let right = (left + block).min(width);
            let (mut blue, mut green, mut red, mut count) = (0u64, 0u64, 0u64, 0u64);
            for row in top..bottom {
                let line = pixels.get(row * width + left..row * width + right);
                for [b, g, r, _] in line.unwrap_or_default() {
                    blue += u64::from(*b);
                    green += u64::from(*g);
                    red += u64::from(*r);
                    count += 1;
                }
            }
            let mean = |sum: u64| u8::try_from(sum / count.max(1)).unwrap_or(u8::MAX);
            let average = [mean(blue), mean(green), mean(red), u8::MAX];
            for row in top..bottom {
                if let Some(line) = out.get_mut(row * width + left..row * width + right) {
                    line.fill(average);
                }
            }
        }
    }
    Image::from_bgra(source.width(), source.height(), out.concat()).ok()
}

type Sum = [u32; 3];

const BLACK: [u8; 4] = [0, 0, 0, u8::MAX];

// Three box blurs in a row come close to a Gaussian one.
fn blurred(source: &Image, radius: u32) -> Option<Image> {
    let width = (source.width() as usize).max(1);
    let radius = radius as usize;
    let mut pixels: Vec<[u8; 4]> = source.bgra().as_chunks::<4>().0.to_vec();
    let mut spare = pixels.clone();
    for _ in 0..3 {
        blur_rows(&pixels, &mut spare, width, radius);
        blur_columns(&spare, &mut pixels, width, radius);
    }
    Image::from_bgra(source.width(), source.height(), pixels.concat()).ok()
}

fn blur_rows(from: &[[u8; 4]], to: &mut [[u8; 4]], width: usize, radius: usize) {
    let scale = reciprocal(radius);
    for (line, out) in from.chunks(width).zip(to.chunks_mut(width)) {
        let last = line.len().saturating_sub(1);
        let at = |index: usize| line.get(index.min(last)).copied().unwrap_or(BLACK);
        let mut sum = times(at(0), radius + 1);
        for index in 1..=radius {
            sum = moved(sum, at(index), [0; 4]);
        }
        for (x, target) in out.iter_mut().enumerate() {
            *target = mean(sum, scale);
            sum = moved(sum, at(x + radius + 1), at(x.saturating_sub(radius)));
        }
    }
}

fn blur_columns(from: &[[u8; 4]], to: &mut [[u8; 4]], width: usize, radius: usize) {
    let scale = reciprocal(radius);
    let rows: Vec<&[[u8; 4]]> = from.chunks(width).collect();
    let last = rows.len().saturating_sub(1);
    let row = |index: usize| rows.get(index.min(last)).copied().unwrap_or_default();
    let mut sums: Vec<Sum> = row(0)
        .iter()
        .map(|&pixel| times(pixel, radius + 1))
        .collect();
    for index in 1..=radius {
        for (sum, &pixel) in sums.iter_mut().zip(row(index)) {
            *sum = moved(*sum, pixel, [0; 4]);
        }
    }
    for (y, out) in to.chunks_mut(width).enumerate() {
        for (target, &sum) in out.iter_mut().zip(&sums) {
            *target = mean(sum, scale);
        }
        let (entering, leaving) = (row(y + radius + 1), row(y.saturating_sub(radius)));
        for ((sum, &entered), &left) in sums.iter_mut().zip(entering).zip(leaving) {
            *sum = moved(*sum, entered, left);
        }
    }
}

fn times([b, g, r, _]: [u8; 4], count: usize) -> Sum {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    [b, g, r].map(|channel| u32::from(channel).saturating_mul(count))
}

// The pixel leaving was counted when it entered, so the sum never goes below it.
fn moved([b, g, r]: Sum, entering: [u8; 4], leaving: [u8; 4]) -> Sum {
    let [b_in, g_in, r_in, _] = entering.map(u32::from);
    let [b_out, g_out, r_out, _] = leaving.map(u32::from);
    [b + b_in - b_out, g + g_in - g_out, r + r_in - r_out]
}

fn reciprocal(radius: usize) -> u64 {
    let window = 2 * radius as u64 + 1;
    ((1 << 24) + window / 2) / window
}

fn mean(sum: Sum, scale: u64) -> [u8; 4] {
    let [b, g, r] = sum
        .map(|total| u8::try_from((u64::from(total) * scale + (1 << 23)) >> 24).unwrap_or(u8::MAX));
    [b, g, r, u8::MAX]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;
    use crate::markup::Colour;

    fn noise(width: u32, height: u32) -> Image {
        let mut state = 0x2545_f491_u32;
        let mut bgra = Vec::new();
        for _ in 0..width * height {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let [b, g, r, _] = state.to_le_bytes();
            bgra.extend_from_slice(&[b, g, r, u8::MAX]);
        }
        Image::from_bgra(width, height, bgra).unwrap()
    }

    fn mark(id: u64, shape: Shape) -> Mark {
        Mark {
            id,
            shape,
            colour: Colour::new(0, 0, 0),
            width: 3.0,
        }
    }

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * image.width() + x) * 4) as usize;
        let bytes = &image.bgra()[at..at + 4];
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }

    #[test]
    fn each_blur_and_pixelation_is_made_over_every_mark_drawn_before_it() {
        let (a, b) = (Point::new(0, 0), Point::new(20, 20));
        let marks = [
            mark(1, Shape::Pen(vec![a, b])),
            mark(2, Shape::Redact(a, b)),
            mark(3, Shape::Blur(a, b)),
            mark(4, Shape::Line(a, b)),
            mark(5, Shape::Pixelate(b, a)),
            mark(6, Shape::Redact(a, b)),
        ];
        let jobs: Vec<(u64, Rect, Vec<u64>)> = jobs(&marks)
            .map(|job| {
                let beneath = job.beneath.iter().map(|mark| mark.id).collect();
                (job.mark.id, job.area, beneath)
            })
            .collect();
        let area = Rect::new(0, 0, 21, 21);
        assert_eq!(jobs, [(3, area, vec![1, 2]), (5, area, vec![1, 2, 3, 4])]);
    }

    #[test]
    fn a_black_box_is_drawn_rather_than_made_from_the_pixels() {
        let source = noise(10, 10);
        let redact = mark(1, Shape::Redact(Point::new(0, 0), Point::new(9, 9)));
        assert_eq!(area_of(&redact), Some(Rect::new(0, 0, 10, 10)));
        assert_eq!(apply(&redact, &source), None);
        assert_eq!(jobs(std::slice::from_ref(&redact)).count(), 0);
    }

    #[test]
    fn pixelation_fills_each_block_with_its_average() {
        let source = noise(10, 7);
        let image = pixelated(&source, 4).unwrap();
        for (block_x, block_y) in [(0, 0), (4, 0), (8, 4)] {
            let first = pixel(&image, block_x, block_y);
            for y in block_y..(block_y + 4).min(7) {
                for x in block_x..(block_x + 4).min(10) {
                    assert_eq!(pixel(&image, x, y), first);
                }
            }
        }
        assert_ne!(pixel(&image, 0, 0), pixel(&image, 4, 0));
    }

    #[test]
    fn a_blur_keeps_a_flat_area_flat_and_smooths_noise() {
        let flat = Image::from_bgra(12, 9, [10, 20, 30, 255].repeat(108)).unwrap();
        assert_eq!(blurred(&flat, 3).unwrap(), flat);
        let source = noise(30, 30);
        let image = blurred(&source, 4).unwrap();
        let spread = |image: &Image| {
            let values: Vec<i32> = image
                .bgra()
                .iter()
                .step_by(4)
                .map(|b| i32::from(*b))
                .collect();
            let mean = values.iter().sum::<i32>() / values.len() as i32;
            values.iter().map(|value| (value - mean).abs()).sum::<i32>()
        };
        assert!(spread(&image) * 3 < spread(&source));
        assert_eq!((image.width(), image.height()), (30, 30));
    }

    fn blurred_slowly(source: &Image, radius: usize) -> Vec<u8> {
        let (width, height) = (source.width() as usize, source.height() as usize);
        let mut pixels: Vec<[u8; 4]> = source.bgra().as_chunks::<4>().0.to_vec();
        let reach = radius as i64;
        for _ in 0..3 {
            for across in [true, false] {
                let before = pixels.clone();
                for y in 0..height {
                    for x in 0..width {
                        let mut sum = [0u32; 3];
                        for offset in -reach..=reach {
                            let (from_x, from_y) = if across {
                                ((x as i64 + offset).clamp(0, width as i64 - 1) as usize, y)
                            } else {
                                (x, (y as i64 + offset).clamp(0, height as i64 - 1) as usize)
                            };
                            let pixel = before[from_y * width + from_x];
                            for (total, value) in sum.iter_mut().zip(pixel) {
                                *total += u32::from(value);
                            }
                        }
                        pixels[y * width + x] = mean(sum, reciprocal(radius));
                    }
                }
            }
        }
        pixels.concat()
    }

    #[test]
    fn a_blur_matches_one_that_adds_up_every_pixel_of_its_window() {
        for (width, height, radius) in [(17, 9, 0), (17, 9, 2), (5, 31, 4), (6, 4, 20), (1, 1, 3)] {
            let source = noise(width, height);
            assert_eq!(
                blurred(&source, radius as u32).unwrap().bgra(),
                blurred_slowly(&source, radius).as_slice(),
                "{width}×{height}, radius {radius}"
            );
        }
    }

    #[test]
    fn a_blur_or_pixelation_covers_its_area_and_other_marks_have_none() {
        let source = noise(4, 4);
        let line = mark(1, Shape::Line(Point::new(0, 0), Point::new(3, 3)));
        assert_eq!(area_of(&line), None);
        assert_eq!(apply(&line, &source), None);
        let pixelate = mark(2, Shape::Pixelate(Point::new(3, 3), Point::new(0, 0)));
        let made = apply(&pixelate, &source).unwrap();
        assert_eq!((made.id, made.area), (2, Rect::new(0, 0, 4, 4)));
        assert_eq!((made.image.width(), made.image.height()), (4, 4));
    }
}
