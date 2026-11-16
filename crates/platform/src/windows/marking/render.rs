//! Draws marks over a part of the captured image in the order they were drawn, on the GPU in
//! tiles the device can hold: for the copied or saved image, and for what lies under a blur or a
//! pixelation before it is made.

use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use bediz_core::markup::effect::{self, Effect};
use bediz_core::markup::{Mark, Shape};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct2D::ID2D1Image;
use windows::core::{Error, Result};

use super::paint::Ink;
use crate::windows::gpu::Gpu;
use crate::windows::readback;

const TILE: u32 = 4096;

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
    for tile in tiles(image.width(), image.height()) {
        let part = crop(&image, tile)?;
        let drawn = draw(gpu, &part, marks, effects, origin, tile)?;
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
) -> Result<Image> {
    let texture = gpu.texture_with(part)?;
    let target = gpu.drawable_bitmap(&texture)?;
    let ink = Ink {
        canvas: &gpu.canvas,
        text: &gpu.text,
        dx: (origin.x + tile.left) as f32,
        dy: (origin.y + tile.top) as f32,
    };
    unsafe {
        gpu.canvas.SetTarget(&target);
        gpu.canvas.BeginDraw();
    }
    let shown = tile.translated(origin.x, origin.y);
    let painted = marks
        .iter()
        .try_for_each(|mark| layer(gpu, &ink, mark, effects, shown));
    let ended = unsafe { gpu.canvas.EndDraw(None, None) };
    unsafe { gpu.canvas.SetTarget(None::<&ID2D1Image>) };
    painted?;
    ended?;
    readback::read(
        gpu,
        &texture,
        Rect::new(0, 0, tile.width() as i32, tile.height() as i32),
    )
}

fn layer(gpu: &Gpu, ink: &Ink, mark: &Mark, effects: &[&Effect], shown: Rect) -> Result<()> {
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
            ink.picture(&bitmap, made.area);
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
    fn a_crop_takes_the_pixels_of_its_area() {
        let bgra: Vec<u8> = (0..16u8).flat_map(|value| [value, 0, 0, 255]).collect();
        let image = Image::from_bgra(4, 4, bgra).unwrap();
        let part = crop(&image, Rect::new(1, 2, 3, 4)).unwrap();
        let blues: Vec<u8> = part.bgra().iter().step_by(4).copied().collect();
        assert_eq!(blues, [9, 10, 13, 14]);
    }
}
