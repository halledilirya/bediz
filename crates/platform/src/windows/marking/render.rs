//! Draws marks over a part of the captured image, or over an opened picture at its own resolution,
//! in the order they were drawn, on the GPU in tiles the device can hold: for the copied or saved
//! image, and for what lies under a blur or a pixelation before it is made.

use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use bediz_core::markup::effect::{self, Effect};
use bediz_core::markup::{Mark, Shape};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct2D::ID2D1Image;
use windows::core::{Error, Result};
use windows_numerics::Matrix3x2;

use super::paint::Ink;
use crate::windows::gpu::Gpu;
use crate::windows::readback;

const TILE: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Scale {
    x: f32,
    y: f32,
}

impl Scale {
    // Marks are kept where they were drawn on the screen; a larger picture takes them to its pixels.
    fn of(image: &Image, area: Rect) -> Self {
        Self {
            x: image.width() as f32 / area.width().max(1) as f32,
            y: image.height() as f32 / area.height().max(1) as f32,
        }
    }

    fn is_none(self) -> bool {
        self.x == 1.0 && self.y == 1.0
    }

    fn on_screen(self, tile: Rect, origin: Point) -> Rect {
        let at = |pixels: i32, scale: f32, start: i32, round: fn(f32) -> f32| {
            start.saturating_add(round(pixels as f32 / scale) as i32)
        };
        Rect::new(
            at(tile.left, self.x, origin.x, f32::floor),
            at(tile.top, self.y, origin.y, f32::floor),
            at(tile.right, self.x, origin.x, f32::ceil),
            at(tile.bottom, self.y, origin.y, f32::ceil),
        )
    }
}

pub fn layered(
    mut image: Image,
    area: Rect,
    marks: &[Mark],
    effects: &[&Effect],
    gpu: &Gpu,
) -> Result<Image> {
    if marks.is_empty() {
        return Ok(image);
    }
    let origin = Point::new(area.left, area.top);
    let scale = Scale::of(&image, area);
    for tile in tiles(image.width(), image.height()) {
        let part = crop(&image, tile)?;
        let drawn = draw(gpu, &part, marks, effects, origin, tile, scale)?;
        image.paste_clipped(&drawn, tile.left, tile.top);
    }
    Ok(image)
}

fn draw(
    gpu: &Gpu,
    part: &Image,
    marks: &[Mark],
    effects: &[&Effect],
    origin: Point,
    tile: Rect,
    scale: Scale,
) -> Result<Image> {
    let texture = gpu.texture_with(part)?;
    let target = gpu.drawable_bitmap(&texture)?;
    let ink = Ink {
        canvas: &gpu.canvas,
        text: &gpu.text,
        dx: origin.x as f32,
        dy: origin.y as f32,
    };
    let transform = Matrix3x2 {
        M11: scale.x,
        M22: scale.y,
        M31: -(tile.left as f32),
        M32: -(tile.top as f32),
        ..Matrix3x2::identity()
    };
    unsafe {
        gpu.canvas.SetTarget(&target);
        gpu.canvas.BeginDraw();
        gpu.canvas.SetTransform(&transform);
    }
    let shown = scale.on_screen(tile, origin);
    let smooth = !scale.is_none();
    let painted = marks
        .iter()
        .try_for_each(|mark| layer(gpu, &ink, mark, effects, shown, smooth));
    let ended = unsafe {
        gpu.canvas.SetTransform(&Matrix3x2::identity());
        gpu.canvas.EndDraw(None, None)
    };
    unsafe { gpu.canvas.SetTarget(None::<&ID2D1Image>) };
    painted?;
    ended?;
    readback::read(
        gpu,
        &texture,
        Rect::new(0, 0, tile.width() as i32, tile.height() as i32),
    )
}

fn layer(
    gpu: &Gpu,
    ink: &Ink,
    mark: &Mark,
    effects: &[&Effect],
    shown: Rect,
    smooth: bool,
) -> Result<()> {
    match mark.shape {
        Shape::Redact(..) => match effect::area_of(mark) {
            Some(area) => ink.cover(area),
            None => Ok(()),
        },
        Shape::Blur(..) | Shape::Pixelate(..) => {
            let made = effects
                .iter()
                .find(|made| made.id == mark.id && made.area.intersection(&shown).is_some());
            let Some(made) = made else {
                return Ok(());
            };
            let bitmap = gpu.memory_bitmap(&made.image)?;
            let blurred = matches!(mark.shape, Shape::Blur(..));
            ink.picture(&bitmap, made.area, smooth && blurred);
            Ok(())
        }
        _ => ink.mark(mark),
    }
}

fn tiles(width: u32, height: u32) -> Vec<Rect> {
    let mut tiles = Vec::new();
    for top in (0..height).step_by(TILE as usize) {
        for left in (0..width).step_by(TILE as usize) {
            let (Ok(x), Ok(y)) = (i32::try_from(left), i32::try_from(top)) else {
                continue;
            };
            let right = left.saturating_add(TILE).min(width);
            let bottom = top.saturating_add(TILE).min(height);
            tiles.push(Rect::new(
                x,
                y,
                i32::try_from(right).unwrap_or(i32::MAX),
                i32::try_from(bottom).unwrap_or(i32::MAX),
            ));
        }
    }
    tiles
}

fn crop(image: &Image, area: Rect) -> Result<Image> {
    let mut part = Image::blank(area.width(), area.height())
        .map_err(|error| Error::new(E_FAIL, error.to_string()))?;
    part.paste_clipped(image, -area.left, -area.top);
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_cover_the_image_once_and_stay_within_the_device_limit() {
        let tiles = tiles(9000, 5000);
        assert_eq!(tiles.len(), 6);
        let covered: u64 = tiles
            .iter()
            .map(|tile| u64::from(tile.width()) * u64::from(tile.height()))
            .sum();
        assert_eq!(covered, 9000 * 5000);
        assert!(
            tiles
                .iter()
                .all(|tile| tile.width() <= TILE && tile.height() <= TILE)
        );
        assert_eq!(tiles.last(), Some(&Rect::new(8192, 4096, 9000, 5000)));
        assert_eq!(super::tiles(10, 10), [Rect::new(0, 0, 10, 10)]);
    }

    #[test]
    fn a_picture_scales_the_marks_to_its_pixels_and_a_screenshot_does_not() {
        let screenshot = Image::blank(300, 200).unwrap();
        let area = Rect::new(-300, 40, 0, 240);
        assert!(Scale::of(&screenshot, area).is_none());
        let tile = Rect::new(0, 0, 300, 200);
        let origin = Point::new(area.left, area.top);
        assert_eq!(
            Scale::of(&screenshot, area).on_screen(tile, origin),
            tile.translated(origin.x, origin.y)
        );
        let picture = Image::blank(900, 400).unwrap();
        let scale = Scale::of(&picture, area);
        assert_eq!(scale, Scale { x: 3.0, y: 2.0 });
        assert_eq!(
            scale.on_screen(Rect::new(400, 100, 900, 400), origin),
            Rect::new(-167, 90, 0, 240)
        );
    }

    #[test]
    fn a_crop_takes_the_pixels_of_its_area() {
        let bgra: Vec<u8> = (0..16u8).flat_map(|value| [value, 0, 0, 255]).collect();
        let image = Image::from_bgra(4, 4, bgra).unwrap();
        let part = crop(&image, Rect::new(1, 2, 3, 4)).unwrap();
        let blues: Vec<u8> = part.bgra().iter().step_by(4).copied().collect();
        assert_eq!(blues, [9, 10, 13, 14]);
    }
}
