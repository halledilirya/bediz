//! Text read from a selection: the image made ready for a recognizer, the words it found placed
//! back on the screen, and the text they make in reading order.

mod page;

pub use page::{Page, Placement, Turn, better_scale, first_scale, prepare};

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

pub fn text(lines: &[Vec<Word>], script: Script) -> String {
    lines
        .iter()
        .map(|line| line_text(line, script))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_text(words: &[Word], script: Script) -> String {
    let words: Vec<&str> = words
        .iter()
        .map(|word| word.text.trim())
        .filter(|word| !word.is_empty())
        .collect();
    match script {
        Script::Spaced => words.join(" "),
        Script::RightToLeft => right_to_left(&words),
        Script::Unspaced => {
            let mut line = String::new();
            for word in words {
                if let (Some(before), Some(after)) = (line.chars().last(), word.chars().next())
                    && spaced(before)
                    && spaced(after)
                {
                    line.push(' ');
                }
                line.push_str(word);
            }
            line
        }
    }
}

// Words come in screen order; a left-to-right run, with the numbers after it, keeps its order.
fn right_to_left<'a>(words: &[&'a str]) -> String {
    let mut runs: Vec<Vec<&'a str>> = Vec::new();
    let mut in_run = false;
    for word in words {
        let left_to_right = reads_left_to_right(word);
        match runs.last_mut() {
            Some(run) if in_run && left_to_right != Some(false) => run.push(word),
            _ => {
                runs.push(vec![word]);
                in_run = left_to_right == Some(true);
            }
        }
    }
    runs.into_iter()
        .rev()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

fn reads_left_to_right(word: &str) -> Option<bool> {
    word.chars()
        .find(|letter| letter.is_alphabetic())
        .map(|letter| !written_right_to_left(letter))
}

fn written_right_to_left(letter: char) -> bool {
    matches!(
        u32::from(letter),
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

fn spaced(letter: char) -> bool {
    letter.is_alphanumeric() && !written_without_spaces(letter)
}

fn written_without_spaces(letter: char) -> bool {
    matches!(
        u32::from(letter),
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF
    )
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
    fn words_are_spaced_and_lines_kept_as_on_the_screen() {
        let lines = [
            line(&["Kaydet", "ve", "kapat"]),
            line(&[" ", ""]),
            line(&["Şubat", "ğ", "ı"]),
        ];
        assert_eq!(text(&lines, Script::Spaced), "Kaydet ve kapat\nŞubat ğ ı");
    }

    #[test]
    fn chinese_and_japanese_lose_the_spaces_between_their_characters_but_not_between_latin_words() {
        let lines = [
            line(&["快", "捷", "键", "指", "南"]),
            line(&["Windows", "の", "設定", "Version", "2", "。"]),
        ];
        assert_eq!(
            text(&lines, Script::Unspaced),
            "快捷键指南\nWindowsの設定Version 2。"
        );
    }

    #[test]
    fn a_right_to_left_line_is_put_back_in_reading_order() {
        let lines = [line(&["عالم", "مرحبا"])];
        assert_eq!(text(&lines, Script::RightToLeft), "مرحبا عالم");
        let between = [line(&["עולם", "2024", "שלום"])];
        assert_eq!(text(&between, Script::RightToLeft), "שלום 2024 עולם");
    }

    #[test]
    fn latin_words_and_their_numbers_in_a_right_to_left_line_keep_their_own_order() {
        let mixed = [line(&["Windows", "11", "Pro", "مرحبا", "عالم"])];
        assert_eq!(
            text(&mixed, Script::RightToLeft),
            "عالم مرحبا Windows 11 Pro"
        );
        let apart = [line(&["Bediz", "في", "PNG", "5"])];
        assert_eq!(text(&apart, Script::RightToLeft), "PNG 5 في Bediz");
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
        assert_eq!(
            text(&rows, Script::Spaced),
            "Kaydet düzelttim. Cargo\nsimgesine Ayarlar\naltında"
        );
        let glued = rows
            .first()
            .and_then(|row| row.get(1))
            .map(|word| word.bounds);
        assert_eq!(glued, Some(placed("", 70.0, 100.0, 95.0, 20.0).bounds));
    }

    #[test]
    fn a_row_given_from_its_right_part_is_joined_in_screen_order() {
        let lines = vec![
            vec![placed("Ayarlar", 150.0, 0.0, 70.0, 20.0)],
            vec![placed("simgesine", 0.0, 0.0, 90.0, 20.0)],
        ];
        assert_eq!(
            text(&rows(lines, Script::Spaced), Script::Spaced),
            "simgesine Ayarlar"
        );
        let arabic = vec![
            vec![placed("مرحبا", 100.0, 0.0, 60.0, 20.0)],
            vec![placed(".", 96.0, 14.0, 4.0, 4.0)],
        ];
        assert_eq!(
            text(&rows(arabic, Script::RightToLeft), Script::RightToLeft),
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
