//! Text read from a selection: the image made ready for a recognizer, the words it found placed
//! back on the screen and joined into the rows they share, the passage they make, and the languages
//! installed since the recognizer was first used.

mod page;
mod passage;

pub use page::{Page, Placement, Turn, better_scale, first_scale, prepare};
pub use passage::{Layout, Passage};

use crate::geometry::Rect;

const SAME_ROW: f32 = 0.5;
const TOUCHING: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Bounds {
    pub fn covering(&self) -> Rect {
        Rect::new(
            self.left.floor() as i32,
            self.top.floor() as i32,
            self.right().ceil() as i32,
            self.bottom().ceil() as i32,
        )
    }

    fn right(&self) -> f32 {
        self.left + self.width
    }

    fn bottom(&self) -> f32 {
        self.top + self.height
    }

    fn union(self, other: Self) -> Self {
        let (left, top) = (self.left.min(other.left), self.top.min(other.top));
        Self {
            left,
            top,
            width: self.right().max(other.right()) - left,
            height: self.bottom().max(other.bottom()) - top,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub bounds: Bounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Spaced,
    Unspaced,
    RightToLeft,
}

impl Script {
    pub fn of(language: &str) -> Self {
        let primary = language
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match primary.as_str() {
            "zh" | "ja" => Self::Unspaced,
            "ar" | "fa" | "he" | "ur" | "yi" => Self::RightToLeft,
            _ => Self::Spaced,
        }
    }
}

// Windows' recognizer can fail on a language installed after the app first used it, until a restart.
pub fn installed_since(first: &[String], now: &[String], asked: Option<&str>) -> bool {
    now.iter()
        .filter(|tag| !first.contains(tag))
        .any(|tag| asked.is_none_or(|asked| tag.eq_ignore_ascii_case(asked)))
}

pub fn prefers_second(first: &[Vec<Word>], second: &[Vec<Word>]) -> bool {
    letters(second) >= letters(first)
}

fn letters(lines: &[Vec<Word>]) -> usize {
    lines
        .iter()
        .flatten()
        .map(|word| {
            word.text
                .chars()
                .filter(|letter| !letter.is_whitespace())
                .count()
        })
        .sum()
}

// The recognizer can split one row of the screen in two at an icon or a boxed word.
pub fn rows(lines: Vec<Vec<Word>>, script: Script) -> Vec<Vec<Word>> {
    let mut rows: Vec<Vec<Word>> = Vec::new();
    for line in lines {
        match rows.last_mut() {
            Some(row) if beside(row, &line) => join(row, line, script),
            _ => rows.push(line),
        }
    }
    rows
}

fn span(words: &[Word]) -> Option<Bounds> {
    words.iter().map(|word| word.bounds).reduce(Bounds::union)
}

fn beside(row: &[Word], line: &[Word]) -> bool {
    let (Some(row), Some(line)) = (span(row), span(line)) else {
        return false;
    };
    let shared = row.bottom().min(line.bottom()) - row.top.max(line.top);
    let apart = line.left >= row.right() || line.right() <= row.left;
    apart && shared >= SAME_ROW * row.height.min(line.height)
}

fn join(row: &mut Vec<Word>, line: Vec<Word>, script: Script) {
    let (Some(before), Some(after)) = (span(row), span(&line)) else {
        row.extend(line);
        return;
    };
    let (mut left, right, left_span, right_span) = if after.right() <= before.left {
        (line, std::mem::take(row), after, before)
    } else {
        (std::mem::take(row), line, before, after)
    };
    let gap = right_span.left - left_span.right();
    let touching = gap < TOUCHING * left_span.height.max(right_span.height);
    let mut right = right.into_iter();
    if touching
        && let Some(last) = left.last_mut()
        && let Some(first) = right.next()
    {
        last.text = match script {
            Script::RightToLeft => format!("{}{}", first.text, last.text),
            Script::Spaced | Script::Unspaced => format!("{}{}", last.text, first.text),
        };
        last.bounds = last.bounds.union(first.bounds);
    }
    left.extend(right);
    *row = left;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(text: &str, left: f32, top: f32, width: f32, height: f32) -> Word {
        Word {
            text: text.to_owned(),
            bounds: Bounds {
                left,
                top,
                width,
                height,
            },
        }
    }

    fn line(words: &[&str]) -> Vec<Word> {
        words
            .iter()
            .map(|text| placed(text, 0.0, 0.0, 1.0, 1.0))
            .collect()
    }

    fn written(lines: Vec<Vec<Word>>, script: Script) -> String {
        Passage::new(lines, script).text(Layout::default())
    }

    #[test]
    fn a_language_tag_tells_how_its_words_are_joined() {
        assert_eq!(Script::of("tr-TR"), Script::Spaced);
        assert_eq!(Script::of("en-US"), Script::Spaced);
        assert_eq!(Script::of("ko-KR"), Script::Spaced);
        assert_eq!(Script::of("zh-Hans-CN"), Script::Unspaced);
        assert_eq!(Script::of("ja"), Script::Unspaced);
        assert_eq!(Script::of("ar-SA"), Script::RightToLeft);
        assert_eq!(Script::of("HE_il"), Script::RightToLeft);
        assert_eq!(Script::of(""), Script::Spaced);
    }

    #[test]
    fn a_box_on_the_screen_covers_every_pixel_its_word_touches() {
        let word = placed("", 10.25, 20.0, 5.5, 3.75);
        assert_eq!(word.bounds.covering(), Rect::new(10, 20, 16, 24));
        assert_eq!(
            placed("", -3.5, -1.0, 1.0, 1.0).bounds.covering(),
            Rect::new(-4, -1, -2, 0)
        );
    }

    #[test]
    fn a_language_installed_after_the_first_reading_is_told_apart() {
        let tags = |tags: &[&str]| tags.iter().map(|tag| (*tag).to_owned()).collect::<Vec<_>>();
        let first = tags(&["ja", "tr"]);
        let now = tags(&["en-US", "tr"]);
        assert!(installed_since(&first, &now, Some("en-US")));
        assert!(installed_since(&first, &now, Some("EN-us")));
        assert!(!installed_since(&first, &now, Some("tr")));
        assert!(!installed_since(&first, &now, Some("ja")));
        assert!(!installed_since(&first, &now, Some("de-DE")));
        assert!(installed_since(&first, &now, None));
        assert!(!installed_since(&first, &first, None));
        assert!(!installed_since(&first, &tags(&["tr"]), None));
        assert!(installed_since(&[], &tags(&["tr"]), None));
        assert!(!installed_since(&[], &[], None));
    }

    #[test]
    fn the_second_reading_is_kept_unless_it_found_less_text() {
        let first = [line(&["Kaydet", "ve", "kapat"])];
        let joined = [line(&["Kaydetve", "kapat"])];
        assert!(prefers_second(&first, &joined));
        let broken = [line(&["Kay", "det"])];
        assert!(!prefers_second(&first, &broken));
        assert!(!prefers_second(&first, &[]));
        assert!(prefers_second(&[], &[]));
    }

    #[test]
    fn a_row_the_recognizer_split_is_joined_again() {
        let lines = vec![
            vec![
                placed("Kaydet", 0.0, 100.0, 60.0, 20.0),
                placed("düzelttim", 70.0, 100.0, 90.0, 20.0),
            ],
            vec![
                placed(".", 161.0, 114.0, 4.0, 4.0),
                placed("Cargo", 170.0, 100.0, 50.0, 20.0),
            ],
            vec![placed("simgesine", 0.0, 130.0, 90.0, 20.0)],
            vec![placed("Ayarlar", 150.0, 131.0, 70.0, 20.0)],
            vec![placed("altında", 0.0, 160.0, 70.0, 20.0)],
        ];
        let rows = rows(lines, Script::Spaced);
        assert_eq!(rows.len(), 3);
        let glued = rows
            .first()
            .and_then(|row| row.get(1))
            .map(|word| word.bounds);
        assert_eq!(glued, Some(placed("", 70.0, 100.0, 95.0, 20.0).bounds));
        assert_eq!(
            written(rows, Script::Spaced),
            "Kaydet düzelttim. Cargo\nsimgesine Ayarlar\naltında"
        );
    }

    #[test]
    fn a_row_given_from_its_right_part_is_joined_in_screen_order() {
        let lines = vec![
            vec![placed("Ayarlar", 150.0, 0.0, 70.0, 20.0)],
            vec![placed("simgesine", 0.0, 0.0, 90.0, 20.0)],
        ];
        assert_eq!(
            written(rows(lines, Script::Spaced), Script::Spaced),
            "simgesine Ayarlar"
        );
        let arabic = vec![
            vec![placed("مرحبا", 100.0, 0.0, 60.0, 20.0)],
            vec![placed(".", 96.0, 14.0, 4.0, 4.0)],
        ];
        assert_eq!(
            written(rows(arabic, Script::RightToLeft), Script::RightToLeft),
            "مرحبا."
        );
    }

    #[test]
    fn lines_above_one_another_or_overlapping_stay_apart() {
        let lines = vec![
            vec![placed("Koyu", 0.0, 0.0, 40.0, 20.0)],
            vec![placed("tema", 0.0, 12.0, 40.0, 20.0)],
            vec![placed("açma", 50.0, 25.0, 40.0, 20.0)],
            vec![],
            vec![placed("seçin", 100.0, 25.0, 40.0, 20.0)],
        ];
        assert_eq!(rows(lines, Script::Spaced).len(), 5);
    }
}
