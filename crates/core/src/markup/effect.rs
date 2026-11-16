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

// Three box blurs in a row come close to a Gaussian one.
fn blurred(source: &Image, radius: u32) -> Option<Image> {
    let (width, height) = (source.width() as usize, source.height() as usize);
    let radius = radius as usize;
    let mut pixels: Vec<[u8; 4]> = source.bgra().as_chunks::<4>().0.to_vec();
    for _ in 0..3 {
        pixels = rows_blurred(&pixels, width, radius);
        pixels = transposed(&pixels, width, height);
        pixels = rows_blurred(&pixels, height, radius);
        pixels = transposed(&pixels, height, width);
    }
    Image::from_bgra(source.width(), source.height(), pixels.concat()).ok()
}

fn rows_blurred(pixels: &[[u8; 4]], width: usize, radius: usize) -> Vec<[u8; 4]> {
    let mut out = Vec::with_capacity(pixels.len());
    for row in pixels.chunks(width.max(1)) {
        blur_line(row, radius, &mut out);
    }
    out
}

fn blur_line(line: &[[u8; 4]], radius: usize, out: &mut Vec<[u8; 4]>) {
    let last = line.len().saturating_sub(1);
    let at = |index: isize| {
        let clamped = usize::try_from(index.max(0)).unwrap_or(0).min(last);
        line.get(clamped).copied().unwrap_or([0, 0, 0, u8::MAX])
    };
    let window = (2 * radius + 1) as u32;
    let radius = isize::try_from(radius).unwrap_or(isize::MAX);
    let (mut blue, mut green, mut red) = (0u32, 0u32, 0u32);
    for index in -radius..=radius {
        let [b, g, r, _] = at(index);
        blue += u32::from(b);
        green += u32::from(g);
        red += u32::from(r);
    }
    for index in 0..line.len() {
        let mean = |sum: u32| u8::try_from(sum / window).unwrap_or(u8::MAX);
        out.push([mean(blue), mean(green), mean(red), u8::MAX]);
        let index = isize::try_from(index).unwrap_or(isize::MAX);
        let [b_out, g_out, r_out, _] = at(index - radius);
        let [b_in, g_in, r_in, _] = at(index + radius + 1);
        blue = blue + u32::from(b_in) - u32::from(b_out);
        green = green + u32::from(g_in) - u32::from(g_out);
        red = red + u32::from(r_in) - u32::from(r_out);
    }
}

fn transposed(pixels: &[[u8; 4]], width: usize, height: usize) -> Vec<[u8; 4]> {
    let mut out = Vec::with_capacity(pixels.len());
    for column in 0..width {
        for row in 0..height {
            out.push(
                pixels
                    .get(row * width + column)
                    .copied()
                    .unwrap_or([0, 0, 0, u8::MAX]),
            );
        }
    }
    out
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
