use crate::geometry::{Point, Rect};
use crate::image::Image;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Blend([u8; 4]),
    Xor { keep: bool, colour: [u8; 3] },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    width: u32,
    height: u32,
    hotspot: Point,
    inks: Vec<Ink>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    origin: Point,
    scale: f32,
    width: u32,
    height: u32,
}

impl Placement {
    pub fn new(area: Rect, width: u32, height: u32) -> Self {
        let scale = if area.width() == 0 {
            1.0
        } else {
            width as f32 / area.width() as f32
        };
        Self {
            origin: Point::new(area.left, area.top),
            scale,
            width,
            height,
        }
    }

    fn point(&self, at: Point) -> Point {
        let along = |position: i32, origin: i32| {
            (position.saturating_sub(origin) as f32 * self.scale).floor() as i32
        };
        Point::new(along(at.x, self.origin.x), along(at.y, self.origin.y))
    }
}

impl Cursor {
    // Windows draws a colour cursor without any alpha through its mask, which can invert the screen.
    pub fn from_colour(
        width: u32,
        height: u32,
        hotspot: Point,
        bgra: &[u8],
        mask: &[bool],
    ) -> Option<Self> {
        let pixels = pixel_count(width, height)?;
        let colours = bgra.as_chunks::<4>().0;
        if colours.len() != pixels {
            return None;
        }
        let inks = if colours.iter().any(|[_, _, _, alpha]| *alpha != 0) {
            colours.iter().map(|&colour| Ink::Blend(colour)).collect()
        } else if mask.len() == pixels {
            colours
                .iter()
                .zip(mask)
                .map(|(&[blue, green, red, _], &keep)| Ink::Xor {
                    keep,
                    colour: [blue, green, red],
                })
                .collect()
        } else {
            return None;
        };
        Some(Self {
            width,
            height,
            hotspot,
            inks,
        })
    }

    pub fn from_monochrome(
        width: u32,
        height: u32,
        hotspot: Point,
        and: &[bool],
        xor: &[bool],
    ) -> Option<Self> {
        let pixels = pixel_count(width, height)?;
        if and.len() != pixels || xor.len() != pixels {
            return None;
        }
        let inks = and
            .iter()
            .zip(xor)
            .map(|(&keep, &flip)| Ink::Xor {
                keep,
                colour: if flip { [u8::MAX; 3] } else { [0; 3] },
            })
            .collect();
        Some(Self {
            width,
            height,
            hotspot,
            inks,
        })
    }

    pub fn shows_in(&self, at: Point, placement: &Placement) -> bool {
        self.placed(placement.point(at), placement.scale)
            .intersection(&bounds(placement.width, placement.height))
            .is_some()
    }

    pub fn draw(&self, image: &mut Image, at: Point, placement: &Placement) {
        let scale = placement.scale;
        let placed = self.placed(placement.point(at), scale);
        let Some(visible) = placed.intersection(&bounds(image.width(), image.height())) else {
            return;
        };
        let row_bytes = image.width() as usize * 4;
        let source = |offset: i32, length: u32| {
            let position = (offset as f32 / scale) as u32;
            position.min(length.saturating_sub(1))
        };
        let pixels = image.bgra_mut();
        for y in visible.top..visible.bottom {
            let source_row = source(y - placed.top, self.height) as usize * self.width as usize;
            for x in visible.left..visible.right {
                let index = source_row + source(x - placed.left, self.width) as usize;
                let start = y as usize * row_bytes + x as usize * 4;
                if let (Some(&ink), Some(pixel)) =
                    (self.inks.get(index), pixels.get_mut(start..start + 4))
                {
                    apply(ink, pixel);
                }
            }
        }
    }

    fn placed(&self, pointer: Point, scale: f32) -> Rect {
        let scaled = |length: u32| ((length as f32 * scale).ceil() as i32).max(1);
        let left = pointer.x - (self.hotspot.x as f32 * scale).round() as i32;
        let top = pointer.y - (self.hotspot.y as f32 * scale).round() as i32;
        Rect::new(
            left,
            top,
            left.saturating_add(scaled(self.width)),
            top.saturating_add(scaled(self.height)),
        )
    }
}

fn pixel_count(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    (width as usize).checked_mul(height as usize)
}

fn bounds(width: u32, height: u32) -> Rect {
    let side = |length: u32| i32::try_from(length).unwrap_or(i32::MAX);
    Rect::new(0, 0, side(width), side(height))
}

fn apply(ink: Ink, pixel: &mut [u8]) {
    match ink {
        Ink::Blend([blue, green, red, alpha]) => {
            for (channel, over) in pixel.iter_mut().zip([blue, green, red]) {
                *channel = blend(*channel, over, alpha);
            }
        }
        Ink::Xor { keep, colour } => {
            for (channel, flip) in pixel.iter_mut().zip(colour) {
                let base = if keep { *channel } else { 0 };
                *channel = base ^ flip;
            }
        }
    }
}

fn blend(under: u8, over: u8, alpha: u8) -> u8 {
    let alpha = u16::from(alpha);
    let mixed = u16::from(over) * alpha + u16::from(under) * (255 - alpha);
    u8::try_from((mixed + 127) / 255).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREY: [u8; 4] = [100, 100, 100, 255];

    fn canvas(width: u32, height: u32) -> Image {
        let pixels = (width * height) as usize;
        Image::from_bgra(width, height, GREY.repeat(pixels)).unwrap()
    }

    fn whole(image: &Image) -> Placement {
        let size = |side: u32| i32::try_from(side).unwrap();
        Placement::new(
            Rect::new(0, 0, size(image.width()), size(image.height())),
            image.width(),
            image.height(),
        )
    }

    fn draw(cursor: &Cursor, image: &mut Image, at: Point) {
        let placement = whole(image);
        cursor.draw(image, at, &placement);
    }

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let start = ((y * image.width() + x) * 4) as usize;
        let mut found = [0; 4];
        found.copy_from_slice(&image.bgra()[start..start + 4]);
        found
    }

    fn solid(width: u32, height: u32, hotspot: Point) -> Cursor {
        let bgra = [0, 0, 255, 255].repeat((width * height) as usize);
        Cursor::from_colour(width, height, hotspot, &bgra, &[]).unwrap()
    }

    #[test]
    fn a_colour_cursor_with_alpha_is_blended_over_the_screen() {
        let half = Cursor::from_colour(1, 1, Point::new(0, 0), &[0, 0, 255, 128], &[]).unwrap();
        let mut image = canvas(3, 3);
        draw(&half, &mut image, Point::new(1, 1));
        assert_eq!(pixel(&image, 1, 1), [50, 50, 178, 255]);
        assert_eq!(pixel(&image, 0, 0), GREY);
    }

    #[test]
    fn a_colour_cursor_without_alpha_is_drawn_through_its_mask() {
        let bgra = [10, 20, 30, 0, 0, 0, 0, 0, 255, 255, 255, 0];
        let cursor =
            Cursor::from_colour(3, 1, Point::new(0, 0), &bgra, &[false, true, true]).unwrap();
        let mut image = canvas(3, 1);
        draw(&cursor, &mut image, Point::new(0, 0));
        assert_eq!(pixel(&image, 0, 0), [10, 20, 30, 255]);
        assert_eq!(pixel(&image, 1, 0), GREY);
        assert_eq!(pixel(&image, 2, 0), [155, 155, 155, 255]);
        assert_eq!(
            Cursor::from_colour(3, 1, Point::new(0, 0), &bgra, &[false]),
            None
        );
    }

    #[test]
    fn a_monochrome_cursor_paints_black_white_screen_or_inverted_screen() {
        let cursor = Cursor::from_monochrome(
            4,
            1,
            Point::new(0, 0),
            &[false, false, true, true],
            &[false, true, false, true],
        )
        .unwrap();
        let mut image = canvas(4, 1);
        draw(&cursor, &mut image, Point::new(0, 0));
        assert_eq!(pixel(&image, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&image, 1, 0), [255, 255, 255, 255]);
        assert_eq!(pixel(&image, 2, 0), GREY);
        assert_eq!(pixel(&image, 3, 0), [155, 155, 155, 255]);
    }

    #[test]
    fn the_hotspot_lands_on_the_pointer() {
        let cursor = solid(2, 2, Point::new(1, 1));
        let mut image = canvas(4, 4);
        draw(&cursor, &mut image, Point::new(2, 2));
        for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
            assert_eq!(pixel(&image, x, y), [0, 0, 255, 255], "{x},{y}");
        }
        assert_eq!(pixel(&image, 3, 3), GREY);
        assert_eq!(pixel(&image, 0, 0), GREY);
    }

    #[test]
    fn a_cursor_over_an_edge_is_cut_and_one_outside_draws_nothing() {
        let cursor = solid(3, 3, Point::new(0, 0));
        let mut image = canvas(2, 2);
        draw(&cursor, &mut image, Point::new(-1, -1));
        assert_eq!(pixel(&image, 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&image, 1, 1), [0, 0, 255, 255]);
        let mut untouched = canvas(2, 2);
        draw(&cursor, &mut untouched, Point::new(-10, 5));
        assert_eq!(untouched, canvas(2, 2));
        let placement = whole(&untouched);
        assert!(!cursor.shows_in(Point::new(-10, 5), &placement));
        assert!(cursor.shows_in(Point::new(-1, -1), &placement));
        assert!(!cursor.shows_in(Point::new(2, 0), &placement));
    }

    #[test]
    fn a_shrunken_recording_shrinks_the_cursor_with_it() {
        let cursor = solid(4, 4, Point::new(2, 2));
        let mut image = canvas(6, 6);
        let half = Placement::new(Rect::new(100, 50, 112, 62), 6, 6);
        cursor.draw(&mut image, Point::new(106, 56), &half);
        for (x, y) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
            assert_eq!(pixel(&image, x, y), [0, 0, 255, 255], "{x},{y}");
        }
        assert_eq!(pixel(&image, 4, 4), GREY);
        assert_eq!(pixel(&image, 1, 1), GREY);
    }

    #[test]
    fn a_cursor_with_no_pixels_or_the_wrong_count_is_refused() {
        assert_eq!(Cursor::from_colour(0, 1, Point::new(0, 0), &[], &[]), None);
        assert_eq!(
            Cursor::from_colour(2, 1, Point::new(0, 0), &[0, 0, 0, 255], &[]),
            None
        );
        assert_eq!(
            Cursor::from_monochrome(2, 1, Point::new(0, 0), &[true], &[true, false]),
            None
        );
    }
}
