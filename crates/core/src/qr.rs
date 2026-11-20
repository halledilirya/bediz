//! QR codes in a selection's image: found and read, light on dark and small ones too, placed
//! back on the screen in reading order, kept apart from the words read around them, and their
//! text as copied and as a label shows it.

use std::collections::HashSet;

use rxing::common::HybridBinarizer;
use rxing::multi::MultipleBarcodeReader;
use rxing::qrcode::cpp_port::QrReader;
use rxing::{BarcodeFormat, BinaryBitmap, DecodeHints, Luma8Source};

use crate::geometry::Rect;
use crate::image::Image;
use crate::notice::Notice;
use crate::ocr::{Turn, Word};

const QUIET_ZONE: usize = 32;
const LARGEST_AREA: f32 = 12_000_000.0;
const ENLARGED_AREA: f32 = 8_000_000.0;
const LARGEST_ENLARGEMENT: f32 = 4.0;
const SMALLEST_ENLARGEMENT: f32 = 1.5;
const SHOWN_LETTERS: usize = 120;
const SHOWN_CODES: usize = 5;
const MORE: char = '\u{2026}';
const ISOLATE: char = '\u{2068}';
const END_ISOLATE: char = '\u{2069}';

#[derive(Clone, Debug, PartialEq)]
pub struct Code {
    pub text: String,
    pub corners: [(f32, f32); 4],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scan {
    pub codes: Vec<Code>,
    pub enlarged: Option<f32>,
    size: (u32, u32),
}

struct Grey {
    width: usize,
    height: usize,
    shades: Vec<u8>,
}

impl Code {
    pub fn covering(&self) -> Rect {
        let (left, top, right, bottom) = self.span();
        Rect::new(
            left.floor() as i32,
            top.floor() as i32,
            right.ceil() as i32,
            bottom.ceil() as i32,
        )
    }

    fn span(&self) -> (f32, f32, f32, f32) {
        self.corners.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(left, top, right, bottom), &(x, y)| {
                (left.min(x), top.min(y), right.max(x), bottom.max(y))
            },
        )
    }

    fn middle(&self) -> (f32, f32) {
        let count = self.corners.len() as f32;
        let (x, y) = self
            .corners
            .iter()
            .fold((0.0, 0.0), |(x, y), &(across, down)| (x + across, y + down));
        (x / count, y / count)
    }

    // rxing lists the corners in no fixed order; a point among all four lies among three of them.
    fn holds(&self, point: (f32, f32)) -> bool {
        let [a, b, c, d] = self.corners;
        [[a, b, c], [a, b, d], [a, c, d], [b, c, d]]
            .into_iter()
            .any(|triangle| within(triangle, point))
    }

    fn same(&self, other: &Code) -> bool {
        self.text == other.text && self.holds(other.middle())
    }
}

fn within([a, b, c]: [(f32, f32); 3], (x, y): (f32, f32)) -> bool {
    let side = |(from_x, from_y): (f32, f32), (to_x, to_y): (f32, f32)| {
        (to_x - from_x) * (y - from_y) - (to_y - from_y) * (x - from_x)
    };
    let sides = [side(a, b), side(b, c), side(c, a)];
    sides.iter().all(|side| *side >= 0.0) || sides.iter().all(|side| *side <= 0.0)
}

impl Scan {
    pub fn placed(self, shown: Rect) -> Vec<Code> {
        let (width, height) = self.size;
        let across = shown.width() as f32 / width.max(1) as f32;
        let down = shown.height() as f32 / height.max(1) as f32;
        self.codes
            .into_iter()
            .map(|code| Code {
                corners: code
                    .corners
                    .map(|(x, y)| (shown.left as f32 + x * across, shown.top as f32 + y * down)),
                text: code.text,
            })
            .collect()
    }
}

pub fn find(image: &Image) -> Scan {
    let area = (image.width() as f32) * (image.height() as f32);
    let fitted = (LARGEST_AREA / area).sqrt().min(1.0);
    let enlarged = Some((ENLARGED_AREA / area).sqrt().min(LARGEST_ENLARGEMENT))
        .filter(|by| *by >= SMALLEST_ENLARGEMENT);
    let grey = Grey::of(image, false);
    let mut found: Vec<Code> = Vec::new();
    let mut keep = |codes: Vec<Code>| {
        for code in codes {
            if !found.iter().any(|seen| seen.same(&code)) {
                found.push(code);
            }
        }
    };
    keep(look(&grey, fitted));
    // QR codes are meant dark on light, but a dark page may show one light on dark.
    keep(look(&Grey::of(image, true), fitted));
    if let Some(by) = enlarged {
        keep(look(&grey, by));
    }
    Scan {
        codes: in_reading_order(found),
        enlarged,
        size: (image.width(), image.height()),
    }
}

// A text recognizer reads letters into a code's dots; they are not text on the screen.
pub fn without_codes(lines: Vec<Vec<Word>>, codes: &[Code], turn: Option<Turn>) -> Vec<Vec<Word>> {
    if codes.is_empty() {
        return lines;
    }
    // Turning by the opposite angle takes a tilted word's level position back onto the screen.
    let onto_screen = turn.map(|turn| Turn {
        degrees: -turn.degrees,
        ..turn
    });
    lines
        .into_iter()
        .map(|line| {
            line.into_iter()
                .filter(|word| {
                    let bounds = word.bounds;
                    let (x, y) = (
                        bounds.left + bounds.width / 2.0,
                        bounds.top + bounds.height / 2.0,
                    );
                    let middle = onto_screen.map_or((x, y), |turn| turn.level(x, y));
                    !codes.iter().any(|code| code.holds(middle))
                })
                .collect::<Vec<_>>()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

// The clipboard's text ends at its first NUL, which would cut the code's text short.
pub fn copied(codes: &[Code]) -> String {
    codes
        .iter()
        .map(|code| {
            code.text
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\0', " ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn notice(codes: &[Code]) -> Option<Notice> {
    match codes {
        [] => None,
        [code] => Some(Notice::CodeCopied(shown(&code.text))),
        several => {
            // A placeable's isolation ends at the first line break, so each line gets its own.
            let mut lines: Vec<String> = several
                .iter()
                .take(SHOWN_CODES)
                .map(|code| format!("{ISOLATE}{}{END_ISOLATE}", shown(&code.text)))
                .collect();
            if several.len() > SHOWN_CODES {
                lines.push(MORE.to_string());
            }
            Some(Notice::CodesCopied(lines.join("\n")))
        }
    }
}

fn look(grey: &Grey, scale: f32) -> Vec<Code> {
    let quiet = QUIET_ZONE as f32;
    let side = |length: usize| (length as f32 * scale).ceil() as usize + 2 * QUIET_ZONE;
    let (width, height) = (side(grey.width), side(grey.height));
    let mut luma = vec![u8::MAX; width.saturating_mul(height)];
    let rows = luma.chunks_mut(width.max(1));
    if scale == 1.0 {
        let lines = grey.shades.chunks(grey.width.max(1));
        for (row, line) in rows.skip(QUIET_ZONE).zip(lines) {
            if let Some(inside) = row.get_mut(QUIET_ZONE..QUIET_ZONE + line.len()) {
                inside.copy_from_slice(line);
            }
        }
    } else {
        for (y, row) in rows.enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                if let Some(shade) = grey.shade(x as f32 - quiet, y as f32 - quiet, scale) {
                    *pixel = shade;
                }
            }
        }
    }
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Vec::new();
    };
    let Ok(source) = Luma8Source::new(luma, width, height) else {
        return Vec::new();
    };
    let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
    let hints = DecodeHints {
        TryHarder: Some(true),
        PossibleFormats: Some(HashSet::from([BarcodeFormat::QR_CODE])),
        ..DecodeHints::default()
    };
    let Ok(results) = QrReader.decode_multiple_with_hints(&mut bitmap, &hints) else {
        return Vec::new();
    };
    let back = |at: f32| (at - quiet) / scale;
    results
        .iter()
        .filter_map(|result| {
            let points = result.getPoints();
            let corner = |at: usize| {
                let point = points.get(at).or(points.last())?;
                Some((back(point.x), back(point.y)))
            };
            Some(Code {
                text: result.getText().to_owned(),
                corners: [corner(0)?, corner(1)?, corner(2)?, corner(3)?],
            })
        })
        .collect()
}

impl Grey {
    // Transparent gaps between monitors read as white either way, the quiet zone a code needs.
    fn of(image: &Image, inverted: bool) -> Self {
        let shades = image
            .bgra()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[blue, green, red, alpha]| {
                let ink =
                    0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue);
                let shade = if inverted {
                    255.0 - ink
                } else {
                    ink + f32::from(u8::MAX - alpha)
                };
                shade.round().clamp(0.0, 255.0) as u8
            })
            .collect();
        Self {
            width: image.width() as usize,
            height: image.height() as usize,
            shades,
        }
    }

    // A small code's dots fall between pixels; enlarging smoothly brings their edges back.
    fn shade(&self, x: f32, y: f32, scale: f32) -> Option<u8> {
        let (width, height) = (self.width as f32 * scale, self.height as f32 * scale);
        if x < 0.0 || y < 0.0 || x >= width || y >= height {
            return None;
        }
        if scale <= 1.0 {
            return self.at((x / scale) as usize, (y / scale) as usize);
        }
        let (across, down) = ((x + 0.5) / scale - 0.5, (y + 0.5) / scale - 0.5);
        let (left, top) = (across.floor(), down.floor());
        let (right_part, lower_part) = (across - left, down - top);
        let pick = |dx: f32, dy: f32| {
            let column = (left + dx).clamp(0.0, self.width.saturating_sub(1) as f32) as usize;
            let row = (top + dy).clamp(0.0, self.height.saturating_sub(1) as f32) as usize;
            f32::from(self.at(column, row).unwrap_or(u8::MAX))
        };
        let upper = pick(0.0, 0.0) * (1.0 - right_part) + pick(1.0, 0.0) * right_part;
        let lower = pick(0.0, 1.0) * (1.0 - right_part) + pick(1.0, 1.0) * right_part;
        Some((upper * (1.0 - lower_part) + lower * lower_part).round() as u8)
    }

    fn at(&self, x: usize, y: usize) -> Option<u8> {
        if x >= self.width {
            return None;
        }
        self.shades.get(y * self.width + x).copied()
    }
}

fn in_reading_order(mut codes: Vec<Code>) -> Vec<Code> {
    codes.sort_by(|a, b| a.span().1.total_cmp(&b.span().1));
    let mut rows: Vec<Vec<Code>> = Vec::new();
    for code in codes {
        match rows.last_mut() {
            Some(row)
                if row
                    .first()
                    .is_some_and(|first| code.middle().1 <= first.span().3) =>
            {
                row.push(code);
            }
            _ => rows.push(vec![code]),
        }
    }
    rows.into_iter()
        .flat_map(|mut row| {
            row.sort_by(|a, b| a.span().0.total_cmp(&b.span().0));
            row
        })
        .collect()
}

// A code can hold line breaks or marks that turn text around; the label shows neither.
fn shown(text: &str) -> String {
    let mut shown = String::new();
    let mut letters = 0;
    let mut gap = false;
    for letter in text.chars() {
        if invisible(letter) {
            continue;
        }
        if letter.is_whitespace() || letter.is_control() {
            gap = !shown.is_empty();
            continue;
        }
        if letters >= SHOWN_LETTERS {
            shown.push(MORE);
            return shown;
        }
        if gap {
            shown.push(' ');
            gap = false;
        }
        shown.push(letter);
        letters += 1;
    }
    shown
}

fn invisible(letter: char) -> bool {
    matches!(
        letter,
        '\u{061C}' | '\u{200B}' | '\u{200E}' | '\u{200F}' | '\u{FEFF}' | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use qrcodegen::{QrCode, QrCodeEcc};

    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const NAVY: [u8; 4] = [60, 30, 20, 255];
    const PALE: [u8; 4] = [235, 240, 245, 255];

    fn filled(width: u32, height: u32, colour: [u8; 4]) -> Image {
        let pixels = (width * height) as usize;
        Image::from_bgra(width, height, colour.repeat(pixels)).unwrap()
    }

    fn code(text: &str, module: u32, ink: [u8; 4], paper: [u8; 4]) -> Image {
        let qr = QrCode::encode_text(text, QrCodeEcc::Medium).unwrap();
        let side = qr.size() as u32 * module;
        let mut image = filled(side, side, paper);
        let dot = filled(module, module, ink);
        for y in 0..qr.size() {
            for x in 0..qr.size() {
                if qr.get_module(x, y) {
                    image.paste_opaque(&dot, x as u32 * module, y as u32 * module);
                }
            }
        }
        image
    }

    fn dotted(text: &str, module: u32) -> Image {
        let qr = QrCode::encode_text(text, QrCodeEcc::Medium).unwrap();
        let size = qr.size();
        let side = size as u32 * module;
        let unit = module as f32;
        let finders = [(0, 0), (size - 7, 0), (0, size - 7)];
        let mut bgra = Vec::new();
        for py in 0..side {
            for px in 0..side {
                let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
                let (column, row) = ((px / module) as i32, (py / module) as i32);
                let finder = finders.iter().find(|&&(left, top)| {
                    (left..left + 7).contains(&column) && (top..top + 7).contains(&row)
                });
                let dark = match finder {
                    Some(&(left, top)) => {
                        let (x, y) = (x - left as f32 * unit, y - top as f32 * unit);
                        let ring = rounded(x, y, 0.0, 7.0 * unit, 2.0 * unit)
                            && !rounded(x, y, unit, 5.0 * unit, 1.5 * unit);
                        ring || rounded(x, y, 2.0 * unit, 3.0 * unit, unit)
                    }
                    None => {
                        let middle = |at: i32| (at as f32 + 0.5) * unit;
                        qr.get_module(column, row)
                            && (x - middle(column)).hypot(y - middle(row)) <= 0.4 * unit
                    }
                };
                bgra.extend(if dark { BLACK } else { WHITE });
            }
        }
        Image::from_bgra(side, side, bgra).unwrap()
    }

    fn rounded(x: f32, y: f32, start: f32, length: f32, radius: f32) -> bool {
        let (x, y) = (x - start, y - start);
        if x < 0.0 || y < 0.0 || x > length || y > length {
            return false;
        }
        let nearest = |at: f32| at.clamp(radius, length - radius);
        (x - nearest(x)).hypot(y - nearest(y)) <= radius
    }

    fn tilted(text: &str, module: u32, quiet: u32) -> Image {
        let qr = QrCode::encode_text(text, QrCodeEcc::Medium).unwrap();
        let half = (qr.size() as u32 * module) as f32 / 2.0;
        let edge = half + quiet as f32;
        let side = (2.0 * edge * std::f32::consts::SQRT_2).ceil() as u32;
        let centre = side as f32 / 2.0;
        let at = |along: f32| ((along + half) / module as f32).floor() as i32;
        let mut bgra = Vec::new();
        for py in 0..side {
            for px in 0..side {
                let (x, y) = (px as f32 + 0.5 - centre, py as f32 + 0.5 - centre);
                let across = (x + y) / std::f32::consts::SQRT_2;
                let down = (y - x) / std::f32::consts::SQRT_2;
                let colour = if across.abs() > edge || down.abs() > edge {
                    NAVY
                } else if qr.get_module(at(across), at(down)) {
                    BLACK
                } else {
                    WHITE
                };
                bgra.extend(colour);
            }
        }
        Image::from_bgra(side, side, bgra).unwrap()
    }

    fn page(width: u32, height: u32, paper: [u8; 4], codes: &[(&Image, u32, u32)]) -> Image {
        let mut page = filled(width, height, paper);
        for (code, x, y) in codes {
            page.paste_opaque(code, *x, *y);
        }
        page
    }

    fn shrunk(image: &Image, by: u32) -> Image {
        let (width, height) = (image.width() / by, image.height() / by);
        let source = image.bgra();
        let mut bgra = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0_u32; 4];
                for dy in 0..by {
                    for dx in 0..by {
                        let at = (((y * by + dy) * image.width() + x * by + dx) * 4) as usize;
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total += u32::from(source[at + channel]);
                        }
                    }
                }
                bgra.extend(sum.map(|total| (total / (by * by)) as u8));
            }
        }
        Image::from_bgra(width, height, bgra).unwrap()
    }

    fn texts(scan: &Scan) -> Vec<&str> {
        scan.codes.iter().map(|code| code.text.as_str()).collect()
    }

    fn placed(text: &str, left: f32, top: f32, side: f32) -> Code {
        Code {
            text: text.to_owned(),
            corners: [
                (left, top),
                (left + side, top),
                (left + side, top + side),
                (left, top + side),
            ],
        }
    }

    fn diamond(text: &str) -> Code {
        Code {
            text: text.to_owned(),
            corners: [(300.0, 0.0), (600.0, 300.0), (0.0, 300.0), (300.0, 600.0)],
        }
    }

    #[test]
    fn a_code_is_read_and_found_where_it_lies() {
        let qr = code("https://example.com/bediz", 4, BLACK, WHITE);
        let side = qr.width() as i32;
        let scan = find(&page(320, 240, WHITE, &[(&qr, 60, 40)]));
        assert_eq!(texts(&scan), ["https://example.com/bediz"]);
        let found = scan.codes.first().unwrap().covering();
        let expected = Rect::new(60, 40, 60 + side, 40 + side);
        for (got, wanted) in [
            (found.left, expected.left),
            (found.top, expected.top),
            (found.right, expected.right),
            (found.bottom, expected.bottom),
        ] {
            assert!((got - wanted).abs() <= 8, "{found:?} against {expected:?}");
        }
    }

    #[test]
    fn a_denser_thumbnail_code_with_dots_off_the_pixel_grid_is_read() {
        let text = "https://example.com/bediz/qr-thumbnail-3";
        let sharp = page(224, 224, WHITE, &[(&code(text, 6, BLACK, WHITE), 25, 25)]);
        let thumbnail = shrunk(&sharp, 4);
        let scan = find(&page(160, 120, WHITE, &[(&thumbnail, 50, 30)]));
        assert_eq!(texts(&scan), [text]);
    }

    #[test]
    fn a_light_code_on_a_dark_page_is_read() {
        let qr = code("dark mode", 4, PALE, NAVY);
        let scan = find(&page(240, 200, NAVY, &[(&qr, 50, 40)]));
        assert_eq!(texts(&scan), ["dark mode"]);
    }

    #[test]
    fn a_code_with_two_pixel_modules_is_read() {
        let qr = code("small", 2, BLACK, WHITE);
        let scan = find(&page(120, 100, WHITE, &[(&qr, 30, 20)]));
        assert_eq!(texts(&scan), ["small"]);
        assert_eq!(scan.enlarged, Some(LARGEST_ENLARGEMENT));
    }

    #[test]
    fn a_small_blurred_code_as_in_a_thumbnail_is_read() {
        let sharp = page(
            232,
            232,
            WHITE,
            &[(&code("thumbnail", 8, BLACK, WHITE), 32, 32)],
        );
        let thumbnail = shrunk(&sharp, 5);
        let scan = find(&page(200, 120, WHITE, &[(&thumbnail, 60, 30)]));
        assert_eq!(texts(&scan), ["thumbnail"]);
    }

    #[test]
    fn a_code_on_a_transparent_gap_is_read() {
        let qr = code("between monitors", 3, BLACK, WHITE);
        let scan = find(&page(200, 160, [0, 0, 0, 0], &[(&qr, 40, 30)]));
        assert_eq!(texts(&scan), ["between monitors"]);
    }

    #[test]
    fn a_light_code_beside_a_transparent_gap_is_read() {
        let qr = code("beside a gap", 4, PALE, NAVY);
        let mut beside = page(300, 220, [0, 0, 0, 0], &[]);
        beside.paste_opaque(&filled(220, 220, NAVY), 80, 0);
        beside.paste_opaque(&qr, 84, 60);
        assert_eq!(texts(&find(&beside)), ["beside a gap"]);
    }

    #[test]
    fn several_codes_are_read_row_by_row() {
        let first = code("first", 3, BLACK, WHITE);
        let second = code("second", 3, BLACK, WHITE);
        let third = code("third", 3, BLACK, WHITE);
        let scan = find(&page(
            420,
            380,
            WHITE,
            &[(&second, 240, 20), (&first, 30, 34), (&third, 30, 220)],
        ));
        assert_eq!(texts(&scan), ["first", "second", "third"]);
    }

    #[test]
    fn a_code_only_a_later_pass_reads_is_kept_beside_one_read_first() {
        let dark = code("dark", 4, BLACK, WHITE);
        let light = code("light", 4, PALE, NAVY);
        let mut both = page(440, 200, WHITE, &[(&dark, 20, 40)]);
        both.paste_opaque(&filled(170, 170, NAVY), 240, 15);
        both.paste_opaque(&light, 280, 40);
        assert_eq!(texts(&find(&both)), ["dark", "light"]);
    }

    #[test]
    fn a_light_code_in_the_corner_beside_a_tilted_code_is_kept_apart_from_it() {
        let text = "https://example.com/bediz/a-tilted-code-beside-a-light-one-in-its-corner";
        let light = code("light", 4, PALE, NAVY);
        let both = page(
            440,
            440,
            NAVY,
            &[(&tilted(text, 6, 18), 40, 40), (&light, 36, 36)],
        );
        assert_eq!(texts(&find(&both)), ["light", text]);
    }

    #[test]
    fn a_code_read_in_several_passes_is_listed_once() {
        let qr = code("once", 4, BLACK, WHITE);
        let scan = find(&page(200, 160, WHITE, &[(&qr, 40, 30)]));
        assert_eq!(texts(&scan), ["once"]);
    }

    #[test]
    fn a_code_in_turkish_is_read_as_written() {
        let qr = code("Bediz: çığ, şöyle, ÜÇ", 4, BLACK, WHITE);
        let scan = find(&page(240, 200, WHITE, &[(&qr, 40, 30)]));
        assert_eq!(texts(&scan), ["Bediz: çığ, şöyle, ÜÇ"]);
    }

    #[test]
    fn a_code_drawn_with_dots_and_rounded_corners_is_read() {
        let text = "https://example.com/bediz/dotted-style";
        let scan = find(&page(300, 300, WHITE, &[(&dotted(text, 6), 40, 40)]));
        assert_eq!(texts(&scan), [text]);
    }

    #[test]
    fn a_page_without_a_code_or_with_one_blacked_out_gives_nothing() {
        let mut busy = filled(300, 200, WHITE);
        busy.paste_opaque(&filled(80, 40, BLACK), 20, 20);
        busy.paste_opaque(&filled(30, 30, NAVY), 150, 90);
        assert!(find(&busy).codes.is_empty());
        let qr = code("hidden", 4, BLACK, WHITE);
        let mut covered = page(240, 200, WHITE, &[(&qr, 40, 30)]);
        covered.paste_opaque(&filled(120, 70, BLACK), 30, 20);
        assert!(find(&covered).codes.is_empty());
    }

    #[test]
    fn codes_are_put_back_where_the_selection_shows_them() {
        let scan = Scan {
            codes: vec![placed("x", 20.0, 10.0, 40.0)],
            enlarged: None,
            size: (200, 100),
        };
        let codes = scan.placed(Rect::new(1000, 500, 1100, 550));
        assert_eq!(codes, [placed("x", 1010.0, 505.0, 20.0)]);
        assert_eq!(
            codes.first().map(Code::covering),
            Some(Rect::new(1010, 505, 1030, 525))
        );
    }

    #[test]
    fn codes_beside_one_another_are_taken_left_to_right() {
        let ordered = in_reading_order(vec![
            placed("c", 0.0, 200.0, 50.0),
            placed("b", 300.0, 0.0, 50.0),
            placed("a", 0.0, 20.0, 50.0),
        ]);
        let order: Vec<&str> = ordered.iter().map(|code| code.text.as_str()).collect();
        assert_eq!(order, ["a", "b", "c"]);
    }

    fn word(text: &str, left: f32, top: f32) -> Word {
        Word {
            text: text.to_owned(),
            bounds: crate::ocr::Bounds {
                left,
                top,
                width: 30.0,
                height: 10.0,
            },
        }
    }

    fn kept_texts(lines: &[Vec<Word>]) -> Vec<Vec<&str>> {
        lines
            .iter()
            .map(|line| line.iter().map(|word| word.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn words_read_inside_a_code_are_dropped() {
        let lines = vec![
            vec![word("Scan", 0.0, 0.0), word("me", 40.0, 0.0)],
            vec![word("x7", 110.0, 120.0), word("beside", 220.0, 120.0)],
            vec![word("#8", 120.0, 150.0)],
        ];
        let codes = [placed("link", 100.0, 100.0, 100.0)];
        let kept = without_codes(lines.clone(), &codes, None);
        assert_eq!(kept_texts(&kept), [vec!["Scan", "me"], vec!["beside"]]);
        assert_eq!(without_codes(lines.clone(), &[], None), lines);
    }

    #[test]
    fn a_word_in_the_corner_beside_a_tilted_code_is_kept() {
        let lines = vec![vec![word("corner", 505.0, 55.0), word("dot", 285.0, 295.0)]];
        let kept = without_codes(lines, &[diamond("link")], None);
        assert_eq!(kept_texts(&kept), [vec!["corner"]]);
    }

    #[test]
    fn only_the_same_text_read_again_where_a_code_lies_is_taken_for_it() {
        let first = diamond("link");
        let again = Code {
            corners: first.corners.map(|(x, y)| (x + 2.0, y - 1.5)),
            ..first.clone()
        };
        assert!(first.same(&again));
        assert!(!first.same(&placed("other", 480.0, 20.0, 80.0)));
        assert!(!first.same(&placed("link", 480.0, 20.0, 80.0)));
        assert!(!first.same(&Code {
            text: "other".to_owned(),
            ..again
        }));
    }

    #[test]
    fn tilted_words_are_turned_onto_the_screen_before_they_are_matched_with_a_code() {
        let tilt = Turn {
            degrees: 90.0,
            x: 0.0,
            y: 0.0,
        };
        let lines = vec![vec![
            word("x7", 135.0, -155.0),
            word("beside", 135.0, 145.0),
        ]];
        let codes = [placed("link", 100.0, 100.0, 100.0)];
        let kept = without_codes(lines, &codes, Some(tilt));
        assert_eq!(kept_texts(&kept), [vec!["beside"]]);
    }

    #[test]
    fn the_codes_are_copied_one_per_line() {
        let codes = [placed("a\r\nb", 0.0, 0.0, 1.0), placed("c", 0.0, 0.0, 1.0)];
        assert_eq!(copied(&codes), "a\nb\nc");
        assert_eq!(copied(&[]), "");
        let odd = [placed("old\rmac\0rest", 0.0, 0.0, 1.0)];
        assert_eq!(copied(&odd), "old\nmac rest");
    }

    #[test]
    fn the_label_shows_what_a_code_says_on_one_line() {
        assert_eq!(notice(&[]), None);
        let tricky = placed(
            " WIFI:S:home;\r\n\tP:\u{202E}sec\u{061C}ret\u{200B};; ",
            0.0,
            0.0,
            1.0,
        );
        assert_eq!(
            notice(&[tricky]),
            Some(Notice::CodeCopied("WIFI:S:home; P:secret;;".to_owned()))
        );
        let long = placed(&"a".repeat(SHOWN_LETTERS + 5), 0.0, 0.0, 1.0);
        let Some(Notice::CodeCopied(shown)) = notice(&[long]) else {
            panic!("one code gives one line");
        };
        assert_eq!(shown.chars().count(), SHOWN_LETTERS + 1);
        assert!(shown.ends_with(MORE));
    }

    #[test]
    fn the_label_lists_several_codes_up_to_a_few() {
        let line = |text: &str| format!("{ISOLATE}{text}{END_ISOLATE}");
        let two = [placed("a", 0.0, 0.0, 1.0), placed("b", 0.0, 0.0, 1.0)];
        assert_eq!(
            notice(&two),
            Some(Notice::CodesCopied(format!("{}\n{}", line("a"), line("b"))))
        );
        let many: Vec<Code> = (0..SHOWN_CODES + 2)
            .map(|at| placed(&at.to_string(), 0.0, 0.0, 1.0))
            .collect();
        let first_five: Vec<String> = (0..SHOWN_CODES).map(|at| line(&at.to_string())).collect();
        assert_eq!(
            notice(&many),
            Some(Notice::CodesCopied(format!(
                "{}\n{MORE}",
                first_five.join("\n")
            )))
        );
    }
}
