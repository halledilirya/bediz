//! The selection's image made ready for a recognizer: grey, dark on light, padded and enlarged so
//! its words come out about 40 pixels high, and the way from a word it found back to the screen.

use super::{Bounds, Word};
use crate::geometry::Rect;
use crate::image::Image;

const FIRST_SCALE: f32 = 2.0;
const EMPTY_SCALE: f32 = 3.0;
const LARGEST_SCALE: f32 = 4.0;
const LARGEST_AREA: f32 = 40_000_000.0;
const IDEAL_WORD_HEIGHT: f32 = 40.0;
const WORTH_ANOTHER_PASS: f32 = 1.25;
const BORDER: u32 = 8;
const SMALLEST_SIDE: u32 = 80;
const MIDDLE_GREY: u8 = 128;
const SHADES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    scale: f32,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    pub degrees: f32,
    pub x: f32,
    pub y: f32,
}

impl Placement {
    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn turned(self, degrees: f32) -> Self {
        Self {
            angle: degrees,
            ..self
        }
    }

    pub fn on_screen(&self, bounds: Bounds, shown: Rect) -> Rect {
        let (left, top) = self.point_on_screen(bounds.left, bounds.top, shown);
        let (right, bottom) = self.point_on_screen(
            bounds.left + bounds.width,
            bounds.top + bounds.height,
            shown,
        );
        Rect::new(
            left.floor() as i32,
            top.floor() as i32,
            right.ceil() as i32,
            bottom.ceil() as i32,
        )
    }

    // The recognizer gives the words of tilted text as if the page were turned back level.
    pub fn turn(&self, shown: Rect) -> Option<Turn> {
        if self.angle == 0.0 || !self.angle.is_finite() {
            return None;
        }
        let centre = (self.width as f32 / 2.0, self.height as f32 / 2.0);
        let (x, y) = self.point_on_screen(centre.0, centre.1, shown);
        Some(Turn {
            degrees: self.angle,
            x,
            y,
        })
    }

    fn point_on_screen(&self, x: f32, y: f32, shown: Rect) -> (f32, f32) {
        let read_width = self.source_width as f32 * self.scale;
        let read_height = self.source_height as f32 * self.scale;
        (
            shown.left as f32 + (x - self.left as f32) * shown.width() as f32 / read_width,
            shown.top as f32 + (y - self.top as f32) * shown.height() as f32 / read_height,
        )
    }
}

pub struct Page {
    grey: Grey,
    placement: Placement,
    columns: Vec<Taps>,
    rows: Vec<Taps>,
}

impl Page {
    pub fn placement(&self) -> Placement {
        self.placement
    }

    pub fn paint(&self, into: &mut [u8], stride: usize) {
        let width = self.placement.width as usize;
        if width == 0 || stride < width {
            return;
        }
        let (left, top) = (self.placement.left as usize, self.placement.top as usize);
        let background = self.grey.background;
        let mut resampled: Vec<(usize, Vec<f32>)> = Vec::new();
        let mut line = vec![0.0; self.columns.len()];
        let rows = into.chunks_mut(stride).take(self.placement.height as usize);
        for (row, out) in rows.enumerate() {
            let Some(out) = out.get_mut(..width) else {
                continue;
            };
            let Some(taps) = row
                .checked_sub(top)
                .and_then(|inside| self.rows.get(inside))
            else {
                out.fill(background);
                continue;
            };
            let [nearest, ..] = taps.at;
            resampled.retain(|(source, _)| *source >= nearest);
            line.fill(0.0);
            for (source, weight) in taps.at.into_iter().zip(taps.weights) {
                if !resampled.iter().any(|(kept, _)| *kept == source) {
                    resampled.push((source, self.across(source)));
                }
                if let Some((_, shades)) = resampled.iter().find(|(kept, _)| *kept == source) {
                    for (total, shade) in line.iter_mut().zip(shades) {
                        *total += weight * shade;
                    }
                }
            }
            let (before, rest) = out.split_at_mut(left.min(width));
            before.fill(background);
            let (inside, after) = rest.split_at_mut(line.len().min(rest.len()));
            for (pixel, total) in inside.iter_mut().zip(&line) {
                *pixel = total.round().clamp(0.0, 255.0) as u8;
            }
            after.fill(background);
        }
    }

    fn across(&self, row: usize) -> Vec<f32> {
        let width = self.grey.width as usize;
        let start = row * width;
        let shades = self
            .grey
            .shades
            .get(start..start + width)
            .unwrap_or_default();
        self.columns
            .iter()
            .map(|taps| {
                taps.at
                    .into_iter()
                    .zip(taps.weights)
                    .map(|(at, weight)| {
                        weight * shades.get(at).map_or(0.0, |shade| f32::from(*shade))
                    })
                    .sum()
            })
            .collect()
    }
}

pub fn first_scale(image: &Image, largest: u32) -> f32 {
    fit(image.width(), image.height(), largest, FIRST_SCALE)
}

// Windows' recognizer reads words best at about 40 pixels high, as Text Grab measured.
pub fn better_scale(lines: &[Vec<Word>], placement: &Placement, largest: u32) -> Option<f32> {
    let mut heights: Vec<f32> = lines
        .iter()
        .flatten()
        .map(|word| word.bounds.height / placement.scale)
        .filter(|height| *height > 0.0)
        .collect();
    heights.sort_by(f32::total_cmp);
    let wanted = match heights.get(heights.len() / 2) {
        Some(middle) => (IDEAL_WORD_HEIGHT / middle)
            .round()
            .clamp(1.0, LARGEST_SCALE),
        None => EMPTY_SCALE,
    };
    let scale = fit(
        placement.source_width,
        placement.source_height,
        largest,
        wanted,
    );
    (scale >= placement.scale * WORTH_ANOTHER_PASS).then_some(scale)
}

pub fn prepare(image: &Image, scale: f32) -> Page {
    let scaled = |side: u32| ((side as f32 * scale).round() as u32).max(1);
    let (width, height) = (scaled(image.width()), scaled(image.height()));
    let canvas = |side: u32| side.saturating_add(2 * BORDER).max(SMALLEST_SIDE);
    let (canvas_width, canvas_height) = (canvas(width), canvas(height));
    let placement = Placement {
        scale,
        left: (canvas_width - width) / 2,
        top: (canvas_height - height) / 2,
        width: canvas_width,
        height: canvas_height,
        source_width: image.width(),
        source_height: image.height(),
        angle: 0.0,
    };
    Page {
        grey: Grey::of(image),
        placement,
        columns: Taps::along(width, scale, image.width()),
        rows: Taps::along(height, scale, image.height()),
    }
}

fn fit(width: u32, height: u32, largest: u32, wanted: f32) -> f32 {
    let longest = width.max(height).max(1) as f32;
    let room = largest.saturating_sub(2 * BORDER).max(1) as f32;
    let padded = |side: u32| side as f32 + 2.0 * BORDER as f32;
    let area = (LARGEST_AREA / (padded(width) * padded(height))).sqrt();
    let scale = wanted.min(room / longest).min(area);
    // A whole scale draws a letter alike wherever the selection starts.
    if scale >= 1.0 { scale.floor() } else { scale }
}

struct Grey {
    width: u32,
    shades: Vec<u8>,
    background: u8,
}

impl Grey {
    fn of(image: &Image) -> Self {
        let (width, height) = (image.width(), image.height());
        let pixels = image.bgra().as_chunks::<4>().0;
        let under = backdrop(pixels, width, height);
        let mut shades: Vec<u8> = pixels.iter().map(|pixel| shade(*pixel, under)).collect();
        let edge_shades = edge(width, height).filter_map(|at| shades.get(at).copied());
        let mut background = median(edge_shades).unwrap_or(u8::MAX);
        // Text recognizers are tuned for dark text on a light page.
        if background < MIDDLE_GREY {
            for shade in &mut shades {
                *shade = u8::MAX - *shade;
            }
            background = u8::MAX - background;
        }
        Self {
            width,
            shades,
            background,
        }
    }
}

#[derive(Clone, Copy)]
struct Taps {
    at: [usize; 4],
    weights: [f32; 4],
}

impl Taps {
    fn along(count: u32, scale: f32, side: u32) -> Vec<Self> {
        let last = i64::from(side.max(1) - 1);
        (0..count)
            .map(|at| {
                let source = at as f32 / scale + 0.5 / scale - 0.5;
                let first = source.floor();
                let offset = source - first;
                Self {
                    at: std::array::from_fn(|tap| {
                        (first as i64 + tap as i64 - 1).clamp(0, last) as usize
                    }),
                    weights: std::array::from_fn(|tap| catmull_rom(offset - (tap as f32 - 1.0))),
                }
            })
            .collect()
    }
}

// Gaps take the background around them; a transparent image gets a page its ink shows on.
fn backdrop(pixels: &[[u8; 4]], width: u32, height: u32) -> u8 {
    let opaque = |&[blue, green, red, alpha]: &[u8; 4]| {
        (alpha == u8::MAX).then(|| shade([blue, green, red, alpha], 0))
    };
    let around = median(
        edge(width, height)
            .filter_map(|at| pixels.get(at))
            .filter_map(opaque),
    );
    around.unwrap_or_else(|| match median(pixels.iter().filter_map(opaque)) {
        Some(ink) if ink >= MIDDLE_GREY => 0,
        _ => u8::MAX,
    })
}

fn shade([blue, green, red, alpha]: [u8; 4], under: u8) -> u8 {
    let behind = f32::from(u8::MAX - alpha) / f32::from(u8::MAX);
    let ink = 0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue);
    (ink + f32::from(under) * behind).round().clamp(0.0, 255.0) as u8
}

fn edge(width: u32, height: u32) -> impl Iterator<Item = usize> {
    let (width, height) = (width as usize, height as usize);
    let last = height.saturating_sub(1);
    let bottom = (last > 0).then_some(last * width);
    let rows = std::iter::once(0)
        .chain(bottom)
        .flat_map(move |start| start..start + width);
    let sides = (1..last).flat_map(move |row| {
        let start = row * width;
        std::iter::once(start).chain((width > 1).then(|| start + width - 1))
    });
    rows.chain(sides)
}

fn median(shades: impl Iterator<Item = u8>) -> Option<u8> {
    let mut counts = [0_usize; SHADES];
    for shade in shades {
        if let Some(count) = counts.get_mut(usize::from(shade)) {
            *count += 1;
        }
    }
    let total: usize = counts.iter().sum();
    if total == 0 {
        return None;
    }
    let mut seen = 0;
    counts
        .iter()
        .position(|count| {
            seen += count;
            seen > total / 2
        })
        .and_then(|shade| u8::try_from(shade).ok())
}

fn catmull_rom(distance: f32) -> f32 {
    let distance = distance.abs();
    if distance < 1.0 {
        (1.5 * distance - 2.5) * distance * distance + 1.0
    } else if distance < 2.0 {
        ((-0.5 * distance + 2.5) * distance - 4.0) * distance + 2.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(width: u32, height: u32, pixel: [u8; 4]) -> Image {
        let bgra = pixel.repeat(width as usize * height as usize);
        Image::from_bgra(width, height, bgra).unwrap()
    }

    fn painted(page: &Page) -> Vec<u8> {
        let (width, height) = page.placement().size();
        let mut shades = vec![0; width as usize * height as usize];
        page.paint(&mut shades, width as usize);
        shades
    }

    fn centre_of(page: &Page) -> usize {
        let placement = page.placement();
        (placement.top as usize + 2) * placement.width as usize + placement.left as usize + 2
    }

    fn word_of_height(height: f32) -> Word {
        Word {
            text: "a".to_owned(),
            bounds: Bounds {
                left: 0.0,
                top: 0.0,
                width: 1.0,
                height,
            },
        }
    }

    #[test]
    fn a_selection_is_enlarged_by_whole_steps_and_shrunk_only_past_the_limit() {
        assert_eq!(
            first_scale(&filled(1_000, 400, [255; 4]), 10_000),
            FIRST_SCALE
        );
        assert_eq!(fit(500, 300, 10_000, 3.0), 3.0);
        assert_eq!(fit(3_000, 300, 10_000, 4.0), 3.0);
        assert_eq!(fit(3_840, 2_160, 10_000, 3.0), 2.0);
        let wider = fit(20_000, 10, 10_000, FIRST_SCALE);
        assert!(wider < 1.0);
        assert!((20_000.0 * wider).round() as u32 + 2 * BORDER <= 10_000);
    }

    #[test]
    fn a_letter_is_drawn_alike_wherever_the_selection_starts() {
        let dot = |size: u32, (left, top): (u32, u32)| {
            let bgra = (0..size * size)
                .flat_map(|pixel| {
                    let (x, y) = (pixel % size, pixel / size);
                    let inked = (left..left + 2).contains(&x) && (top..top + 2).contains(&y);
                    if inked { [0, 0, 0, 255] } else { [255; 4] }
                })
                .collect();
            Image::from_bgra(size, size, bgra).unwrap()
        };
        let around = |image: &Image, (left, top): (u32, u32)| {
            let page = prepare(image, 3.0);
            let placement = page.placement();
            let shades = painted(&page);
            let width = placement.width as usize;
            let first_column = (placement.left + (left - 2) * 3) as usize;
            let first_row = (placement.top + (top - 2) * 3) as usize;
            (first_row..first_row + 18)
                .flat_map(|row| {
                    let start = row * width + first_column;
                    shades.get(start..start + 18).unwrap().to_vec()
                })
                .collect::<Vec<u8>>()
        };
        let here = around(&dot(9, (3, 3)), (3, 3));
        let there = around(&dot(12, (6, 4)), (6, 4));
        assert!(here.contains(&0));
        assert_eq!(here.len(), there.len());
        assert!(
            here.iter()
                .zip(&there)
                .all(|(one, other)| one.abs_diff(*other) <= 1)
        );
    }

    #[test]
    fn a_whole_screen_is_never_enlarged_past_the_largest_area() {
        for (width, height) in [(3_840, 2_160), (5_120, 2_880), (11_520, 2_160)] {
            let scale = fit(width, height, 10_000, EMPTY_SCALE);
            let side = |side: u32| (side as f32 * scale).round() + 2.0 * BORDER as f32;
            assert!(
                side(width) * side(height) <= LARGEST_AREA,
                "{width}x{height}"
            );
        }
        assert_eq!(fit(1_000, 400, 10_000, EMPTY_SCALE), EMPTY_SCALE);
    }

    #[test]
    fn small_words_are_read_again_larger_and_large_ones_are_not() {
        let placement = prepare(&filled(100, 50, [255; 4]), FIRST_SCALE).placement();
        let read = |heights: &[f32]| {
            [heights
                .iter()
                .map(|height| word_of_height(height * FIRST_SCALE))
                .collect::<Vec<_>>()]
        };
        let small = read(&[8.0, 9.0]);
        assert_eq!(
            better_scale(&small, &placement, 10_000),
            Some(LARGEST_SCALE)
        );
        let usual = read(&[12.0, 13.5]);
        assert_eq!(better_scale(&usual, &placement, 10_000), Some(3.0));
        let medium = read(&[18.0]);
        assert_eq!(better_scale(&medium, &placement, 10_000), None);
        let large = read(&[30.0, 90.0]);
        assert_eq!(better_scale(&large, &placement, 10_000), None);
        assert_eq!(better_scale(&[], &placement, 10_000), Some(EMPTY_SCALE));
        assert_eq!(better_scale(&small, &placement, 200), None);
    }

    #[test]
    fn a_wide_selection_is_prepared_within_the_recognizer_limit() {
        let image = filled(20_000, 10, [10, 20, 30, 255]);
        let (width, height) = prepare(&image, first_scale(&image, 10_000))
            .placement()
            .size();
        assert!(width <= 10_000);
        assert!(height >= SMALLEST_SIDE);
    }

    #[test]
    fn a_small_selection_is_padded_with_its_own_shade() {
        let image = filled(10, 4, [200, 220, 240, 255]);
        let page = prepare(&image, FIRST_SCALE);
        let (width, height) = page.placement().size();
        assert_eq!(page.placement().scale(), FIRST_SCALE);
        assert!(width >= SMALLEST_SIDE && height >= SMALLEST_SIDE);
        assert!(painted(&page).iter().all(|shade| *shade == 224));
    }

    #[test]
    fn transparent_gaps_between_monitors_take_the_selection_s_shade() {
        let mut bgra = [0, 0, 0, 0].repeat(12);
        bgra.splice(0..4, [240, 230, 220, 255]);
        let image = Image::from_bgra(4, 3, bgra).unwrap();
        let page = prepare(&image, FIRST_SCALE);
        assert!(painted(&page).iter().all(|shade| *shade == 228));
    }

    #[test]
    fn text_on_a_transparent_image_reaches_the_recognizer_dark_on_light() {
        for ink in [[0, 0, 0, 255], [255, 255, 255, 255]] {
            let mut bgra = [0, 0, 0, 0].repeat(25);
            bgra.splice(48..52, ink);
            let page = prepare(&Image::from_bgra(5, 5, bgra).unwrap(), 1.0);
            let shades = painted(&page);
            assert_eq!(shades.get(centre_of(&page)), Some(&0), "{ink:?}");
            assert_eq!(shades.first(), Some(&255), "{ink:?}");
        }
    }

    #[test]
    fn light_text_on_a_dark_background_reaches_the_recognizer_dark_on_light() {
        let mut bgra = [30, 30, 30, 255].repeat(25);
        bgra.splice(48..52, [255, 255, 255, 255]);
        let dark = prepare(&Image::from_bgra(5, 5, bgra).unwrap(), 1.0);
        let inverted = painted(&dark);
        assert_eq!(inverted.get(centre_of(&dark)), Some(&0));
        assert_eq!(inverted.first(), Some(&225));
        let mut bgra = [250, 250, 250, 255].repeat(25);
        bgra.splice(48..52, [0, 0, 0, 255]);
        let light = prepare(&Image::from_bgra(5, 5, bgra).unwrap(), 1.0);
        let kept = painted(&light);
        assert_eq!(kept.get(centre_of(&light)), Some(&0));
        assert_eq!(kept.first(), Some(&250));
    }

    #[test]
    fn an_unscaled_selection_keeps_its_shades() {
        let shade = |pixel: u32| 128 + (pixel % 127) as u8;
        let bgra = (0..7_000u32 * 2)
            .flat_map(|pixel| [shade(pixel), shade(pixel), shade(pixel), 255])
            .collect();
        let page = prepare(&Image::from_bgra(7_000, 2, bgra).unwrap(), 1.0);
        let placement = page.placement();
        let start = placement.top as usize * placement.width as usize + placement.left as usize;
        let expected: Vec<u8> = (0..7_000).map(shade).collect();
        assert_eq!(
            painted(&page).get(start..start + 7_000),
            Some(expected.as_slice())
        );
    }

    #[test]
    fn a_page_is_painted_row_by_row_into_a_wider_stride() {
        let page = prepare(&filled(10, 4, [200, 220, 240, 255]), 1.0);
        let (width, height) = page.placement().size();
        let stride = width as usize + 3;
        let mut into = vec![7; stride * height as usize];
        page.paint(&mut into, stride);
        for row in into.chunks(stride) {
            let (shades, padding) = row.split_at(width as usize);
            assert!(shades.iter().all(|shade| *shade == 224));
            assert!(padding.iter().all(|byte| *byte == 7));
        }
        let mut short = vec![7; width as usize - 1];
        page.paint(&mut short, width as usize - 1);
        assert!(short.iter().all(|byte| *byte == 7));
    }

    #[test]
    fn a_word_s_box_lands_on_the_screen_where_it_was_read() {
        let placement = prepare(&filled(100, 50, [255; 4]), 3.0).placement();
        let (left, top) = (placement.left as f32, placement.top as f32);
        let read = Bounds {
            left: left + 30.0,
            top: top + 60.0,
            width: 60.0,
            height: 30.0,
        };
        let shown = Rect::new(1_000, 500, 1_100, 550);
        assert_eq!(
            placement.on_screen(read, shown),
            Rect::new(1_010, 520, 1_030, 530)
        );
        let halved = Rect::new(0, 0, 50, 25);
        assert_eq!(placement.on_screen(read, halved), Rect::new(5, 10, 15, 15));
        let in_the_padding = Bounds {
            left: 0.0,
            top: 0.0,
            width: 2.0,
            height: 2.0,
        };
        assert_eq!(
            placement.on_screen(in_the_padding, shown),
            Rect::new(997, 497, 998, 498)
        );
    }

    #[test]
    fn tilted_words_turn_around_the_page_s_centre_as_shown_on_the_screen() {
        let placement = prepare(&filled(100, 50, [255; 4]), 2.0).placement();
        let shown = Rect::new(-1_100, 500, -1_000, 550);
        assert_eq!(placement.turn(shown), None);
        assert_eq!(placement.turned(f32::NAN).turn(shown), None);
        assert_eq!(
            placement.turned(10.0).turn(shown),
            Some(Turn {
                degrees: 10.0,
                x: -1_050.0,
                y: 525.0
            })
        );
        let halved = Rect::new(0, 0, 50, 25);
        assert_eq!(
            placement.turned(-7.5).turn(halved),
            Some(Turn {
                degrees: -7.5,
                x: 25.0,
                y: 12.5
            })
        );
    }
}
