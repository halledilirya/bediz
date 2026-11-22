//! An image opened to be marked up, from a file or the clipboard: why it could not be opened, the
//! size its header gives, its pixels made upright and premultiplied, a clipboard bitmap made
//! whole, and where it is shown.

use std::borrow::Cow;

use thiserror::Error;

use crate::geometry::Rect;
use crate::image::{Image, ImageError};

pub const MOST_PIXELS: u64 = 1 << 27;
// Eight bytes a pixel holds the largest image that opens, even stored uncompressed.
const MOST_SOURCE_BYTES: u64 = MOST_PIXELS * 8;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
const CHECKER_LIGHT: u8 = 0xFF;
const CHECKER_DARK: u8 = 0xCC;
const FILE_HEADER: usize = 14;
const INFO_HEADER: u32 = 40;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum OpenError {
    #[error("the clipboard holds no image")]
    NoImage,
    #[error("another app is using the clipboard")]
    ClipboardBusy,
    #[error("the image is {width}x{height}, more pixels than can be opened")]
    TooLarge { width: u32, height: u32 },
    #[error("the image could not be read: {0}")]
    Unreadable(String),
    #[error("{0}")]
    Other(String),
    #[error("{0}")]
    Unexpected(String),
}

impl OpenError {
    pub fn locale_id(&self) -> &'static str {
        match self {
            Self::NoImage => "open-error-no-image",
            Self::ClipboardBusy => "open-error-clipboard-busy",
            Self::TooLarge { .. } => "open-error-too-large",
            Self::Unreadable(_) => "open-error-unreadable",
            Self::Other(_) => "open-error-other",
            Self::Unexpected(_) => "open-error-unexpected",
        }
    }
}

pub fn check_size(width: u32, height: u32) -> Result<(), OpenError> {
    if width == 0 || height == 0 {
        return Err(OpenError::Unreadable(format!(
            "the image is {width}x{height}"
        )));
    }
    if u64::from(width) * u64::from(height) > MOST_PIXELS {
        return Err(OpenError::TooLarge { width, height });
    }
    Ok(())
}

pub fn check_source(length: usize) -> Result<(), OpenError> {
    if u64::try_from(length).is_ok_and(|length| length <= MOST_SOURCE_BYTES) {
        return Ok(());
    }
    Err(OpenError::Unreadable(format!(
        "{length} bytes are more than an image that opens takes"
    )))
}

pub fn shown_on(width: u32, height: u32, room: Rect) -> Option<Rect> {
    if width == 0 || height == 0 || room.is_empty() {
        return None;
    }
    let fit = (f64::from(room.width()) / f64::from(width))
        .min(f64::from(room.height()) / f64::from(height))
        .min(1.0);
    let wide = ((f64::from(width) * fit).round() as u32).clamp(1, room.width());
    let tall = ((f64::from(height) * fit).round() as u32).clamp(1, room.height());
    let left = room.left.saturating_add(half(room.width() - wide));
    let top = room.top.saturating_add(half(room.height() - tall));
    Some(Rect::new(
        left,
        top,
        left.saturating_add(i32::try_from(wide).ok()?),
        top.saturating_add(i32::try_from(tall).ok()?),
    ))
}

fn half(length: u32) -> i32 {
    i32::try_from(length / 2).unwrap_or(i32::MAX)
}

pub fn premultiplied(width: u32, height: u32, mut bgra: Vec<u8>) -> Result<Image, ImageError> {
    for pixel in bgra.as_chunks_mut::<4>().0 {
        let [blue, green, red, alpha] = *pixel;
        if alpha < u8::MAX {
            *pixel = [
                times(blue, alpha),
                times(green, alpha),
                times(red, alpha),
                alpha,
            ];
        }
    }
    Image::from_bgra(width, height, bgra)
}

// Some apps leave a bitmap's alpha byte unused at zero, so one with no visible pixel is opaque.
pub fn fill_unused_alpha(bgra: &mut [u8]) {
    let pixels = bgra.as_chunks_mut::<4>().0;
    if pixels.iter().all(|[_, _, _, alpha]| *alpha == 0) {
        for [_, _, _, alpha] in pixels {
            *alpha = u8::MAX;
        }
    }
}

fn times(level: u8, alpha: u8) -> u8 {
    u8::try_from((u16::from(level) * u16::from(alpha) + 127) / 255).unwrap_or(u8::MAX)
}

// The values are Exif's orientations: how the stored pixels turn to stand upright.
pub fn upright(image: &Image, orientation: u16) -> Option<Image> {
    if !(2..=8).contains(&orientation) {
        return None;
    }
    let (width, height) = (image.width() as usize, image.height() as usize);
    let (out_width, out_height) = if orientation >= 5 {
        (height, width)
    } else {
        (width, height)
    };
    let pixels = image.bgra().as_chunks::<4>().0;
    let mut out = Vec::with_capacity(image.bgra().len());
    for y in 0..out_height {
        for x in 0..out_width {
            let (from_x, from_y) = match orientation {
                2 => (width - 1 - x, y),
                3 => (width - 1 - x, height - 1 - y),
                4 => (x, height - 1 - y),
                5 => (y, x),
                6 => (y, height - 1 - x),
                7 => (width - 1 - y, height - 1 - x),
                _ => (width - 1 - y, x),
            };
            out.extend_from_slice(pixels.get(from_y * width + from_x)?);
        }
    }
    Image::from_bgra(
        u32::try_from(out_width).ok()?,
        u32::try_from(out_height).ok()?,
        out,
    )
    .ok()
}

pub fn halved(image: &Image) -> Option<Image> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let (half_width, half_height) = (width.div_ceil(2), height.div_ceil(2));
    let rows: Vec<&[[u8; 4]]> = image.bgra().as_chunks::<4>().0.chunks(width).collect();
    let mut out = Vec::with_capacity(half_width * half_height * 4);
    for y in 0..half_height {
        let top = *rows.get(2 * y)?;
        let bottom = rows.get(2 * y + 1).copied().unwrap_or(top);
        for x in 0..half_width {
            let (left, right) = (2 * x, (2 * x + 1).min(width - 1));
            let mut sums = [0u16; 4];
            for pixel in [
                top.get(left),
                top.get(right),
                bottom.get(left),
                bottom.get(right),
            ]
            .into_iter()
            .flatten()
            {
                for (sum, level) in sums.iter_mut().zip(pixel) {
                    *sum += u16::from(*level);
                }
            }
            out.extend(sums.map(|sum| u8::try_from((sum + 2) / 4).unwrap_or(u8::MAX)));
        }
    }
    Image::from_bgra(
        u32::try_from(half_width).ok()?,
        u32::try_from(half_height).ok()?,
        out,
    )
    .ok()
}

pub fn within(image: &Image, most: u32) -> Option<Cow<'_, Image>> {
    let most = most.max(1);
    let mut fitted = Cow::Borrowed(image);
    while fitted.width() > most || fitted.height() > most {
        fitted = Cow::Owned(halved(&fitted)?);
    }
    Some(fitted)
}

pub fn checker(square: u32) -> Option<Image> {
    let square = square.max(1);
    let side = square.checked_mul(2)?;
    let mut bgra = Vec::new();
    for y in 0..side {
        for x in 0..side {
            let level = if (x / square + y / square).is_multiple_of(2) {
                CHECKER_LIGHT
            } else {
                CHECKER_DARK
            };
            bgra.extend_from_slice(&[level, level, level, u8::MAX]);
        }
    }
    Image::from_bgra(side, side, bgra).ok()
}

// The PNG format puts its header chunk, with the size, first.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.get(..PNG_SIGNATURE.len())? != PNG_SIGNATURE || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let read = |at| bytes_at(png, at).map(u32::from_be_bytes);
    Some((read(16)?, read(20)?))
}

pub fn bitmap_size(dib: &[u8]) -> Option<(u32, u32)> {
    if read_u32(dib, 0)? < INFO_HEADER {
        return None;
    }
    let width = u32::try_from(bytes_at(dib, 4).map(i32::from_le_bytes)?).ok()?;
    // A negative height is a bitmap stored top row first.
    let height = bytes_at(dib, 8).map(i32::from_le_bytes)?.unsigned_abs();
    Some((width, height))
}

// A clipboard bitmap lacks the file header that image decoders read first.
pub fn bitmap_header(dib: &[u8]) -> Option<[u8; FILE_HEADER]> {
    let size = read_u32(dib, 0)?;
    let bits = bytes_at(dib, 14).map(u16::from_le_bytes)?;
    let compression = read_u32(dib, 16)?;
    let used = read_u32(dib, 32)?;
    if size < INFO_HEADER {
        return None;
    }
    let masks: u64 = match compression {
        BI_BITFIELDS if size == INFO_HEADER => 12,
        BI_ALPHABITFIELDS if size == INFO_HEADER => 16,
        _ => 0,
    };
    let colours = match (used, bits) {
        (0, 1..=8) => 1u64 << bits,
        (0, _) => 0,
        (used, _) => u64::from(used),
    };
    let offset = FILE_HEADER as u64 + u64::from(size) + masks + colours * 4;
    let total = FILE_HEADER.checked_add(dib.len())?;
    let (offset, length) = (u32::try_from(offset).ok()?, u32::try_from(total).ok()?);
    if offset > length {
        return None;
    }
    let parts = [
        b"BM".as_slice(),
        &length.to_le_bytes(),
        &[0; 4],
        &offset.to_le_bytes(),
    ];
    parts.concat().try_into().ok()
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes_at(bytes, at).map(u32::from_le_bytes)
}

fn bytes_at<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(width: u32, height: u32) -> Image {
        let bgra = (0..width * height)
            .flat_map(|index| [index as u8, 0, 0, u8::MAX])
            .collect();
        Image::from_bgra(width, height, bgra).unwrap()
    }

    fn blues(image: &Image) -> Vec<u8> {
        image.bgra().iter().step_by(4).copied().collect()
    }

    #[test]
    fn an_image_that_fits_is_shown_whole_in_the_middle() {
        let room = Rect::new(-1920, 0, 0, 1080);
        assert_eq!(
            shown_on(800, 600, room),
            Some(Rect::new(-1360, 240, -560, 840))
        );
        assert_eq!(shown_on(1920, 1080, room), Some(room));
    }

    #[test]
    fn a_larger_image_is_scaled_down_to_fit_and_keeps_its_shape() {
        let room = Rect::new(0, 0, 1920, 1080);
        let shown = shown_on(4000, 3000, room).unwrap();
        assert_eq!(shown, Rect::new(240, 0, 1680, 1080));
        let shown = shown_on(6000, 100, room).unwrap();
        assert_eq!((shown.width(), shown.height()), (1920, 32));
        assert_eq!(shown.top, 524);
        let shown = shown_on(100_000, 1, room).unwrap();
        assert_eq!((shown.width(), shown.height()), (1920, 1));
    }

    #[test]
    fn nothing_is_shown_for_an_empty_image_or_room() {
        assert_eq!(shown_on(0, 10, Rect::new(0, 0, 10, 10)), None);
        assert_eq!(shown_on(10, 10, Rect::default()), None);
    }

    #[test]
    fn an_image_with_too_many_pixels_is_refused() {
        assert_eq!(check_size(16_384, 8_192), Ok(()));
        assert_eq!(
            check_size(16_384, 8_193),
            Err(OpenError::TooLarge {
                width: 16_384,
                height: 8_193
            })
        );
        assert!(matches!(check_size(0, 10), Err(OpenError::Unreadable(_))));
    }

    #[test]
    fn see_through_pixels_are_premultiplied_and_opaque_ones_kept() {
        let image = premultiplied(2, 1, vec![200, 100, 50, 128, 10, 20, 30, u8::MAX]).unwrap();
        assert_eq!(image.bgra(), [100, 50, 25, 128, 10, 20, 30, 255]);
        let clear = premultiplied(2, 1, vec![200, 100, 50, 0, 1, 2, 3, 0]).unwrap();
        assert_eq!(clear.bgra(), [0; 8]);
        assert!(!clear.is_opaque());
        let faint = premultiplied(2, 1, vec![200, 100, 50, 0, 9, 9, 9, 1]).unwrap();
        assert_eq!(faint.bgra(), [0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn a_bitmap_with_no_visible_pixel_is_made_opaque_and_any_other_kept() {
        let mut unused = vec![200, 100, 50, 0, 1, 2, 3, 0];
        fill_unused_alpha(&mut unused);
        assert_eq!(unused, [200, 100, 50, 255, 1, 2, 3, 255]);
        let mut faint = vec![200, 100, 50, 0, 9, 9, 9, 1];
        fill_unused_alpha(&mut faint);
        assert_eq!(faint, [200, 100, 50, 0, 9, 9, 9, 1]);
    }

    #[test]
    fn each_orientation_stands_the_image_upright() {
        let image = numbered(3, 2);
        let cases: [(u16, (u32, u32), [u8; 6]); 7] = [
            (2, (3, 2), [2, 1, 0, 5, 4, 3]),
            (3, (3, 2), [5, 4, 3, 2, 1, 0]),
            (4, (3, 2), [3, 4, 5, 0, 1, 2]),
            (5, (2, 3), [0, 3, 1, 4, 2, 5]),
            (6, (2, 3), [3, 0, 4, 1, 5, 2]),
            (7, (2, 3), [5, 2, 4, 1, 3, 0]),
            (8, (2, 3), [2, 5, 1, 4, 0, 3]),
        ];
        for (orientation, (width, height), expected) in cases {
            let turned = upright(&image, orientation).unwrap();
            assert_eq!((turned.width(), turned.height()), (width, height));
            assert_eq!(blues(&turned), expected, "orientation {orientation}");
        }
        assert_eq!(upright(&image, 1), None);
        assert_eq!(upright(&image, 9), None);
        let back = upright(&upright(&image, 6).unwrap(), 8).unwrap();
        assert_eq!(back, image);
    }

    #[test]
    fn halving_averages_each_square_and_keeps_an_odd_edge() {
        let image = numbered(3, 3);
        let half = halved(&image).unwrap();
        assert_eq!((half.width(), half.height()), (2, 2));
        assert_eq!(blues(&half), [2, 4, 7, 8]);
        assert_eq!(half.bgra().get(3), Some(&255));
        let single = halved(&numbered(1, 1)).unwrap();
        assert_eq!(single, numbered(1, 1));
    }

    #[test]
    fn a_picture_is_halved_until_both_sides_fit_and_kept_when_they_do() {
        let image = numbered(9, 2);
        let fitted = within(&image, 3).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (3, 1));
        assert!(matches!(within(&image, 9), Some(Cow::Borrowed(kept)) if *kept == image));
        let single = within(&image, 0).unwrap();
        assert_eq!((single.width(), single.height()), (1, 1));
    }

    #[test]
    fn the_checker_alternates_squares_of_its_size() {
        let checker = checker(2).unwrap();
        assert_eq!((checker.width(), checker.height()), (4, 4));
        assert_eq!(
            blues(&checker),
            [
                255, 255, 204, 204, 255, 255, 204, 204, 204, 204, 255, 255, 204, 204, 255, 255
            ]
        );
    }

    fn info_header(size: u32, bits: u16, compression: u32, used: u32) -> Vec<u8> {
        let mut header = vec![0; size.max(INFO_HEADER) as usize];
        header.splice(0..4, size.to_le_bytes());
        header.splice(14..16, bits.to_le_bytes());
        header.splice(16..20, compression.to_le_bytes());
        header.splice(32..36, used.to_le_bytes());
        header
    }

    fn offset_of(file: &[u8]) -> u32 {
        u32::from_le_bytes(file[10..14].try_into().unwrap())
    }

    #[test]
    fn a_clipboard_bitmap_gets_the_header_of_a_bitmap_file() {
        let mut dib = info_header(40, 32, 0, 0);
        dib.extend([1, 2, 3, 4]);
        let header = bitmap_header(&dib).unwrap();
        assert_eq!(&header[0..2], b"BM");
        assert_eq!(u32::from_le_bytes(header[2..6].try_into().unwrap()), 58);
        assert_eq!(&header[6..10], [0; 4]);
        assert_eq!(offset_of(&header), 54);
    }

    #[test]
    fn the_pixels_start_after_the_masks_and_the_colour_table() {
        let with_room = |header: Vec<u8>| {
            let mut dib = header;
            dib.extend(vec![0; 2048]);
            offset_of(&bitmap_header(&dib).unwrap())
        };
        assert_eq!(with_room(info_header(40, 32, BI_BITFIELDS, 0)), 66);
        assert_eq!(with_room(info_header(40, 32, BI_ALPHABITFIELDS, 0)), 70);
        assert_eq!(with_room(info_header(124, 32, BI_BITFIELDS, 0)), 138);
        assert_eq!(with_room(info_header(40, 8, 0, 0)), 54 + 1024);
        assert_eq!(with_room(info_header(40, 8, 0, 16)), 54 + 64);
        assert_eq!(with_room(info_header(40, 24, 0, 0)), 54);
    }

    #[test]
    fn a_bitmap_too_short_for_its_header_or_table_is_refused() {
        assert_eq!(bitmap_header(&[0; 20]), None);
        assert_eq!(bitmap_header(&info_header(12, 24, 0, 0)), None);
        assert_eq!(bitmap_header(&info_header(40, 8, 0, 0)), None);
    }

    #[test]
    fn a_bitmap_gives_its_size_either_way_up() {
        let sized = |width: i32, height: i32| {
            let mut dib = info_header(40, 32, 0, 0);
            dib.splice(4..8, width.to_le_bytes());
            dib.splice(8..12, height.to_le_bytes());
            bitmap_size(&dib)
        };
        assert_eq!(sized(7, 5), Some((7, 5)));
        assert_eq!(sized(7, -5), Some((7, 5)));
        assert_eq!(sized(-7, 5), None);
        assert_eq!(bitmap_size(&info_header(12, 24, 0, 0)), None);
        assert_eq!(bitmap_size(&[40, 0, 0, 0, 7]), None);
    }

    #[test]
    fn a_png_gives_its_size_before_it_is_decoded() {
        let png = numbered(5, 3).encode_png().unwrap();
        assert_eq!(png_size(&png), Some((5, 3)));
        assert_eq!(png_size(&png[..20]), None);
        assert_eq!(png_size(b"GIF89a, not a PNG at all"), None);
    }

    #[test]
    fn a_source_longer_than_the_largest_image_needs_is_refused_by_its_length() {
        let most = usize::try_from(MOST_SOURCE_BYTES).unwrap();
        assert_eq!(check_source(0), Ok(()));
        assert_eq!(check_source(most), Ok(()));
        assert!(matches!(
            check_source(most + 1),
            Err(OpenError::Unreadable(_))
        ));
        assert!(check_source(u32::MAX as usize + 1).is_err());
        assert!(check_source(usize::MAX).is_err());
    }

    #[test]
    fn every_reason_has_its_message() {
        let ids = [
            OpenError::NoImage,
            OpenError::ClipboardBusy,
            OpenError::TooLarge {
                width: 1,
                height: 1,
            },
            OpenError::Unreadable(String::new()),
            OpenError::Other(String::new()),
            OpenError::Unexpected(String::new()),
        ]
        .map(|error| error.locale_id());
        assert_eq!(
            ids,
            [
                "open-error-no-image",
                "open-error-clipboard-busy",
                "open-error-too-large",
                "open-error-unreadable",
                "open-error-other",
                "open-error-unexpected"
            ]
        );
    }
}
