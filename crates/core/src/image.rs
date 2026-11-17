//! BGRA images with premultiplied alpha: joining parts into one, encoding as PNG, the bottom-up
//! rows the clipboard takes and a pixel's colour written as hex.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("an image needs a width and a height above zero, got {width}x{height}")]
    Empty { width: u32, height: u32 },
    #[error("{actual} bytes do not hold a {width}x{height} BGRA image")]
    Size {
        width: u32,
        height: u32,
        actual: usize,
    },
    #[error(transparent)]
    Png(#[from] png::EncodingError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

impl Image {
    pub fn from_bgra(width: u32, height: u32, bgra: Vec<u8>) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::Empty { width, height });
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4));
        if expected != Some(bgra.len()) {
            return Err(ImageError::Size {
                width,
                height,
                actual: bgra.len(),
            });
        }
        Ok(Self {
            width,
            height,
            bgra,
        })
    }

    pub fn blank(width: u32, height: u32) -> Result<Self, ImageError> {
        let length = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(ImageError::Size {
                width,
                height,
                actual: 0,
            })?;
        Self::from_bgra(width, height, vec![0; length])
    }

    pub fn paste_opaque(&mut self, part: &Image, x: u32, y: u32) {
        let row_bytes = self.width as usize * 4;
        let part_row_bytes = part.width as usize * 4;
        let visible_width = part.width.min(self.width.saturating_sub(x)) as usize;
        let start_x = x as usize * 4;
        for (row, source) in part.bgra.chunks_exact(part_row_bytes).enumerate() {
            let target_row = y as usize + row;
            let start = target_row * row_bytes + start_x;
            let (Some(target), Some(source)) = (
                self.bgra.get_mut(start..start + visible_width * 4),
                source.get(..visible_width * 4),
            ) else {
                break;
            };
            for (to, from) in target
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(source.as_chunks::<4>().0)
            {
                let [blue, green, red, _] = *from;
                *to = [blue, green, red, u8::MAX];
            }
        }
    }

    pub fn paste_clipped(&mut self, part: &Image, x: i32, y: i32) {
        let skip_x = usize::try_from(x.saturating_neg().max(0)).unwrap_or(0);
        let skip_y = usize::try_from(y.saturating_neg().max(0)).unwrap_or(0);
        let (Ok(left), Ok(top)) = (usize::try_from(x.max(0)), usize::try_from(y.max(0))) else {
            return;
        };
        let (width, height) = (self.width as usize, self.height as usize);
        let visible = (part.width as usize)
            .saturating_sub(skip_x)
            .min(width.saturating_sub(left));
        if visible == 0 {
            return;
        }
        let part_rows = part.bgra.chunks_exact(part.width as usize * 4).skip(skip_y);
        for (row, source) in (top..height).zip(part_rows) {
            let start = (row * width + left) * 4;
            let (Some(target), Some(source)) = (
                self.bgra.get_mut(start..start + visible * 4),
                source.get(skip_x * 4..(skip_x + visible) * 4),
            ) else {
                break;
            };
            target.copy_from_slice(source);
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bgra(&self) -> &[u8] {
        &self.bgra
    }

    pub(crate) fn bgra_mut(&mut self) -> &mut [u8] {
        &mut self.bgra
    }

    pub fn opaque_bottom_up_bgra(&self) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(self.bgra.len());
        for row in self.bgra.chunks_exact(self.width as usize * 4).rev() {
            for [blue, green, red, _] in row.as_chunks::<4>().0 {
                pixels.extend_from_slice(&[*blue, *green, *red, u8::MAX]);
            }
        }
        pixels
    }

    pub fn is_opaque(&self) -> bool {
        self.bgra
            .as_chunks::<4>()
            .0
            .iter()
            .all(|[_, _, _, alpha]| *alpha == u8::MAX)
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, ImageError> {
        let pixels = self.bgra.as_chunks::<4>().0;
        let (color, samples) = if self.is_opaque() {
            let mut rgb = Vec::with_capacity(pixels.len() * 3);
            for [blue, green, red, _] in pixels {
                rgb.extend_from_slice(&[*red, *green, *blue]);
            }
            (png::ColorType::Rgb, rgb)
        } else {
            let mut rgba = Vec::with_capacity(pixels.len() * 4);
            for &[blue, green, red, alpha] in pixels {
                let straight = |level: u8| unpremultiplied(level, alpha);
                rgba.extend_from_slice(&[straight(red), straight(green), straight(blue), alpha]);
            }
            (png::ColorType::Rgba, rgba)
        };
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, self.width, self.height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_deflate_compression(png::DeflateCompression::Level(2));
        encoder.set_filter(png::Filter::Adaptive);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&samples)?;
        writer.finish()?;
        Ok(png)
    }
}

fn unpremultiplied(level: u8, alpha: u8) -> u8 {
    match alpha {
        0 | u8::MAX => level,
        _ => u8::try_from((u16::from(level) * 255 + u16::from(alpha) / 2) / u16::from(alpha))
            .unwrap_or(u8::MAX),
    }
}

pub fn hex_colour(bgra: [u8; 4]) -> String {
    let [blue, green, red, _] = bgra;
    format!("#{red:02X}{green:02X}{blue:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colour_is_red_green_blue_from_bgra() {
        assert_eq!(hex_colour([0x10, 0xa0, 0xff, 0x00]), "#FFA010");
        assert_eq!(hex_colour([0, 0, 0, 255]), "#000000");
    }

    fn two_by_two() -> Image {
        let top_row = [1, 2, 3, 0, 4, 5, 6, 10];
        let bottom_row = [7, 8, 9, 20, 10, 11, 12, 30];
        Image::from_bgra(2, 2, [top_row, bottom_row].concat()).unwrap()
    }

    #[test]
    fn rejects_empty_sizes() {
        assert!(matches!(
            Image::from_bgra(0, 10, Vec::new()),
            Err(ImageError::Empty { .. })
        ));
        assert!(matches!(
            Image::from_bgra(10, 0, Vec::new()),
            Err(ImageError::Empty { .. })
        ));
    }

    #[test]
    fn rejects_buffers_of_the_wrong_length() {
        assert!(matches!(
            Image::from_bgra(2, 2, vec![0; 15]),
            Err(ImageError::Size { actual: 15, .. })
        ));
        assert!(matches!(
            Image::from_bgra(u32::MAX, u32::MAX, vec![0; 4]),
            Err(ImageError::Size { .. })
        ));
    }

    #[test]
    fn bottom_up_rows_are_reversed_and_opaque() {
        assert_eq!(
            two_by_two().opaque_bottom_up_bgra(),
            vec![7, 8, 9, 255, 10, 11, 12, 255, 1, 2, 3, 255, 4, 5, 6, 255]
        );
    }

    fn decode(png: Vec<u8>) -> (png::ColorType, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut decoded).unwrap();
        decoded.truncate(info.buffer_size());
        (info.color_type, decoded)
    }

    #[test]
    fn pasting_makes_the_part_opaque_and_leaves_gaps_transparent() {
        let mut canvas = Image::blank(3, 2).unwrap();
        canvas.paste_opaque(&two_by_two(), 1, 1);
        let gap = [0, 0, 0, 0];
        let expected = [gap, gap, gap, gap, [1, 2, 3, 255], [4, 5, 6, 255]].concat();
        assert_eq!(canvas.bgra, expected);
    }

    #[test]
    fn pasting_past_the_edge_is_cut_off() {
        let mut canvas = Image::blank(2, 2).unwrap();
        canvas.paste_opaque(&two_by_two(), 1, 1);
        canvas.paste_opaque(&two_by_two(), 5, 5);
        let gap = [0, 0, 0, 0];
        assert_eq!(canvas.bgra, [gap, gap, gap, [1, 2, 3, 255]].concat());
    }

    #[test]
    fn a_clipped_paste_is_cut_on_every_side_and_keeps_the_part_as_it_is() {
        let mut canvas = Image::blank(2, 2).unwrap();
        canvas.paste_clipped(&two_by_two(), -1, -1);
        let gap = [0, 0, 0, 0];
        assert_eq!(canvas.bgra, [[10, 11, 12, 30], gap, gap, gap].concat());
        let mut canvas = Image::blank(2, 2).unwrap();
        canvas.paste_clipped(&two_by_two(), 1, 0);
        assert_eq!(
            canvas.bgra,
            [gap, [1, 2, 3, 0], gap, [7, 8, 9, 20]].concat()
        );
        let untouched = Image::blank(2, 2).unwrap();
        let mut canvas = untouched.clone();
        canvas.paste_clipped(&two_by_two(), 2, 0);
        canvas.paste_clipped(&two_by_two(), -2, 0);
        assert_eq!(canvas, untouched);
    }

    #[test]
    fn png_with_gaps_keeps_them_transparent() {
        let mut canvas = Image::blank(2, 1).unwrap();
        let part = Image::from_bgra(1, 1, vec![10, 20, 30, 0]).unwrap();
        canvas.paste_opaque(&part, 1, 0);
        let (color, decoded) = decode(canvas.encode_png().unwrap());
        assert_eq!(color, png::ColorType::Rgba);
        assert_eq!(decoded, [0, 0, 0, 0, 30, 20, 10, 255]);
    }

    #[test]
    fn a_partly_see_through_pixel_is_written_with_its_own_colour() {
        let image = Image::from_bgra(2, 1, vec![100, 50, 25, 128, 7, 8, 9, 255]).unwrap();
        assert!(!image.is_opaque());
        let (color, decoded) = decode(image.encode_png().unwrap());
        assert_eq!(color, png::ColorType::Rgba);
        assert_eq!(decoded, [50, 100, 199, 128, 9, 8, 7, 255]);
    }

    #[test]
    fn png_round_trips_as_rgb() {
        let mut canvas = Image::blank(2, 2).unwrap();
        canvas.paste_opaque(&two_by_two(), 0, 0);
        let (color, decoded) = decode(canvas.encode_png().unwrap());
        assert_eq!(color, png::ColorType::Rgb);
        assert_eq!(decoded, [3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10]);
    }

    #[test]
    fn png_of_a_large_synthetic_frame_is_valid() {
        let (width, height) = (640, 360);
        let mut bgra = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                bgra.extend_from_slice(&[
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x + y) % 256) as u8,
                    255,
                ]);
            }
        }
        let image = Image::from_bgra(width as u32, height as u32, bgra).unwrap();
        let png = image.encode_png().unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().width, 640);
        assert_eq!(reader.info().height, 360);
    }
}
