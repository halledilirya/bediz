//! The words read from a selection in reading order: the word nearest the pointer, and the text of
//! the words chosen or of them all, its lines kept, joined or indented as on the screen.

use std::ops::RangeInclusive;

use super::{Bounds, Script, Word, span};

const WIDEST_LETTER: f32 = 1.25;
const NARROWEST_LETTER: f32 = 1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub join_lines: bool,
    pub keep_indentation: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    lines: Vec<Vec<Word>>,
    script: Script,
}

struct Written {
    start: f32,
    from_start: bool,
    text: String,
}

impl Passage {
    pub fn new(lines: Vec<Vec<Word>>, script: Script) -> Self {
        let lines = lines
            .into_iter()
            .map(|line| {
                let mut kept: Vec<Word> = line
                    .into_iter()
                    .map(|word| Word {
                        text: word.text.trim().to_owned(),
                        bounds: word.bounds,
                    })
                    .filter(|word| !word.text.is_empty())
                    .collect();
                if script == Script::RightToLeft {
                    kept.reverse();
                }
                kept
            })
            .filter(|line| !line.is_empty())
            .collect();
        Self { lines, script }
    }

    pub fn words(&self) -> impl Iterator<Item = &Word> {
        self.lines.iter().flatten()
    }

    pub fn len(&self) -> usize {
        self.lines.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    // Past the end of a short line the pointer stays on it, as in a text editor.
    pub fn nearest(&self, x: f32, y: f32) -> Option<usize> {
        let mut first = 0;
        let mut closest: Option<((f32, f32), usize, &[Word])> = None;
        for line in &self.lines {
            if let Some(bounds) = span(line) {
                let away = (
                    apart(y, bounds.top, bounds.bottom()),
                    apart(x, bounds.left, bounds.right()),
                );
                if closest.as_ref().is_none_or(|(nearer, ..)| away < *nearer) {
                    closest = Some((away, first, line));
                }
            }
            first += line.len();
        }
        let (_, first, line) = closest?;
        line.iter()
            .map(|word| apart(x, word.bounds.left, word.bounds.right()))
            .enumerate()
            .min_by(|(_, one), (_, other)| one.total_cmp(other))
            .map(|(at, _)| first + at)
    }

    pub fn under(&self, x: f32, y: f32, reach: f32) -> Option<usize> {
        self.words()
            .map(|word| {
                let bounds = word.bounds;
                let dx = apart(x, bounds.left, bounds.right());
                let dy = apart(y, bounds.top, bounds.bottom());
                (dx, dy)
            })
            .enumerate()
            .filter(|(_, (dx, dy))| *dx <= reach && *dy <= reach)
            .min_by(|(_, one), (_, other)| one.0.hypot(one.1).total_cmp(&other.0.hypot(other.1)))
            .map(|(at, _)| at)
    }

    pub fn text(&self, layout: Layout) -> String {
        self.write(&(0..=usize::MAX), layout)
    }

    pub fn chosen_text(&self, chosen: RangeInclusive<usize>, layout: Layout) -> String {
        self.write(&chosen, layout)
    }

    fn write(&self, chosen: &RangeInclusive<usize>, layout: Layout) -> String {
        let mut first = 0;
        let mut written = Vec::new();
        for line in &self.lines {
            let words: Vec<&str> = self
                .in_reading_order(line)
                .into_iter()
                .filter(|(at, _)| chosen.contains(&(first + at)))
                .map(|(_, text)| text)
                .collect();
            let from_start = chosen.contains(&first);
            first += line.len();
            if let (false, Some(bounds)) = (words.is_empty(), span(line)) {
                written.push(Written {
                    start: self.start_of(bounds),
                    from_start,
                    text: self.line_text(&words),
                });
            }
        }
        if layout.join_lines {
            let mut joined = String::new();
            for line in &written {
                self.append(&mut joined, &line.text);
            }
            return joined;
        }
        let advance = layout.keep_indentation.then(|| self.advance()).flatten();
        let starts = written.iter().map(|line| line.start);
        let base = match self.script {
            Script::RightToLeft => starts.reduce(f32::max),
            Script::Spaced | Script::Unspaced => starts.reduce(f32::min),
        };
        written
            .iter()
            .map(|line| {
                let spaces = match (advance, base) {
                    (Some(advance), Some(base)) if line.from_start => {
                        ((line.start - base).abs() / advance).round() as usize
                    }
                    _ => 0,
                };
                format!("{}{}", " ".repeat(spaces), line.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn start_of(&self, bounds: Bounds) -> f32 {
        match self.script {
            Script::RightToLeft => bounds.right(),
            Script::Spaced | Script::Unspaced => bounds.left,
        }
    }

    // A right-to-left line is put in order whole, so the words chosen keep the order it gives them.
    fn in_reading_order<'a>(&self, line: &'a [Word]) -> Vec<(usize, &'a str)> {
        let words = line.iter().map(|word| word.text.as_str()).enumerate();
        match self.script {
            Script::RightToLeft => right_to_left(words.rev().collect()),
            Script::Spaced | Script::Unspaced => words.collect(),
        }
    }

    fn line_text(&self, words: &[&str]) -> String {
        match self.script {
            Script::Spaced | Script::RightToLeft => words.join(" "),
            Script::Unspaced => {
                let mut line = String::new();
                for word in words {
                    self.append(&mut line, word);
                }
                line
            }
        }
    }

    fn append(&self, text: &mut String, more: &str) {
        let between = match (text.chars().last(), more.chars().next()) {
            (Some(before), Some(after)) => {
                self.script != Script::Unspaced || (spaced(before) && spaced(after))
            }
            _ => false,
        };
        if between {
            text.push(' ');
        }
        text.push_str(more);
    }

    // A word's box leaves out the space beside its end letters; the slope over lengths does not.
    fn advance(&self) -> Option<f32> {
        let sizes: Vec<(f32, f32)> = self
            .words()
            .map(|word| (word.text.chars().count() as f32, word.bounds.width))
            .collect();
        let letters: f32 = sizes.iter().map(|(letters, _)| letters).sum();
        let width: f32 = sizes.iter().map(|(_, width)| width).sum();
        let mean = width / letters;
        // Under a pixel a letter no text could have been read, and the spaces would have no bound.
        if !(mean.is_finite() && mean >= NARROWEST_LETTER) {
            return None;
        }
        let count = sizes.len() as f32;
        let (letters, width) = (letters / count, width / count);
        let spread: f32 = sizes.iter().map(|(each, _)| (each - letters).powi(2)).sum();
        let shared: f32 = sizes
            .iter()
            .map(|(each, wide)| (each - letters) * (wide - width))
            .sum();
        let slope = shared / spread;
        Some(if slope.is_finite() {
            slope.clamp(mean, mean * WIDEST_LETTER)
        } else {
            mean
        })
    }
}

fn apart(value: f32, low: f32, high: f32) -> f32 {
    (low - value).max(value - high).max(0.0)
}

// Words come in screen order; a left-to-right run, with the numbers after it, keeps its order.
fn right_to_left<'a>(words: Vec<(usize, &'a str)>) -> Vec<(usize, &'a str)> {
    let mut runs: Vec<Vec<(usize, &'a str)>> = Vec::new();
    let mut in_run = false;
    for word in words {
        let left_to_right = reads_left_to_right(word.1);
        match runs.last_mut() {
            Some(run) if in_run && left_to_right != Some(false) => run.push(word),
            _ => {
                runs.push(vec![word]);
                in_run = left_to_right == Some(true);
            }
        }
    }
    runs.into_iter().rev().flatten().collect()
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

    const LINES: Layout = Layout {
        join_lines: false,
        keep_indentation: false,
    };
    const JOINED: Layout = Layout {
        join_lines: true,
        keep_indentation: false,
    };
    const INDENTED: Layout = Layout {
        join_lines: false,
        keep_indentation: true,
    };

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

    fn texts(passage: &Passage) -> Vec<&str> {
        passage.words().map(|word| word.text.as_str()).collect()
    }

    // Each word's box falls a pixel short at both ends, as the recognizer's boxes do.
    fn code(rows: &[(usize, &[&str])]) -> Passage {
        let lines = rows
            .iter()
            .enumerate()
            .map(|(row, (indent, words))| {
                let mut column = *indent;
                words
                    .iter()
                    .map(|text| {
                        let letters = text.chars().count();
                        let left = column as f32 * 10.0 + 1.0;
                        column += letters + 1;
                        placed(
                            text,
                            left,
                            row as f32 * 20.0,
                            letters as f32 * 10.0 - 2.0,
                            14.0,
                        )
                    })
                    .collect()
            })
            .collect();
        Passage::new(lines, Script::Spaced)
    }

    #[test]
    fn words_are_spaced_and_lines_kept_as_on_the_screen() {
        let passage = Passage::new(
            vec![
                line(&["Kaydet", " ve ", "kapat"]),
                line(&[" ", ""]),
                line(&["Şubat", "ğ", "ı"]),
            ],
            Script::Spaced,
        );
        assert_eq!(passage.text(LINES), "Kaydet ve kapat\nŞubat ğ ı");
        assert_eq!(passage.len(), 6);
        assert!(!passage.is_empty());
        assert!(Passage::new(vec![line(&[" "]), vec![]], Script::Spaced).is_empty());
    }

    #[test]
    fn chinese_and_japanese_lose_the_spaces_between_their_characters_but_not_between_latin_words() {
        let passage = Passage::new(
            vec![
                line(&["快", "捷", "键", "指", "南"]),
                line(&["Windows", "の", "設定", "Version", "2", "。"]),
            ],
            Script::Unspaced,
        );
        assert_eq!(passage.text(LINES), "快捷键指南\nWindowsの設定Version 2。");
    }

    #[test]
    fn a_right_to_left_line_is_put_back_in_reading_order() {
        let passage = Passage::new(vec![line(&["عالم", "مرحبا"])], Script::RightToLeft);
        assert_eq!(passage.text(LINES), "مرحبا عالم");
        assert_eq!(texts(&passage), ["مرحبا", "عالم"]);
        let between = Passage::new(vec![line(&["עולם", "2024", "שלום"])], Script::RightToLeft);
        assert_eq!(between.text(LINES), "שלום 2024 עולם");
    }

    #[test]
    fn latin_words_and_their_numbers_in_a_right_to_left_line_keep_their_own_order() {
        let mixed = Passage::new(
            vec![line(&["Windows", "11", "Pro", "مرحبا", "عالم"])],
            Script::RightToLeft,
        );
        assert_eq!(mixed.text(LINES), "عالم مرحبا Windows 11 Pro");
        let apart = Passage::new(
            vec![line(&["Bediz", "في", "PNG", "5"])],
            Script::RightToLeft,
        );
        assert_eq!(apart.text(LINES), "PNG 5 في Bediz");
    }

    #[test]
    fn words_chosen_from_a_right_to_left_line_keep_the_order_the_whole_line_gives_them() {
        let passage = Passage::new(
            vec![vec![
                placed("Windows", 0.0, 0.0, 70.0, 20.0),
                placed("11", 80.0, 0.0, 20.0, 20.0),
                placed("Pro", 110.0, 0.0, 30.0, 20.0),
                placed("مرحبا", 150.0, 0.0, 50.0, 20.0),
                placed("عالم", 210.0, 0.0, 40.0, 20.0),
            ]],
            Script::RightToLeft,
        );
        let (eleven, pro) = (passage.nearest(90.0, 10.0), passage.nearest(125.0, 10.0));
        assert_eq!((eleven, pro), (Some(3), Some(2)));
        for layout in [LINES, JOINED] {
            assert_eq!(passage.chosen_text(2..=3, layout), "11 Pro");
        }
        assert_eq!(passage.chosen_text(3..=3, LINES), "11");
        assert_eq!(passage.chosen_text(2..=2, LINES), "Pro");
        assert_eq!(passage.chosen_text(0..=1, LINES), "عالم مرحبا");
        assert_eq!(passage.text(LINES), "عالم مرحبا Windows 11 Pro");
    }

    #[test]
    fn the_word_nearest_the_pointer_is_found_along_the_line_level_with_it() {
        let passage = Passage::new(
            vec![
                vec![
                    placed("Kısa", 0.0, 0.0, 40.0, 20.0),
                    placed("satır", 50.0, 2.0, 40.0, 16.0),
                ],
                vec![
                    placed("Daha", 0.0, 30.0, 40.0, 20.0),
                    placed("uzun", 50.0, 30.0, 40.0, 20.0),
                    placed("bir", 100.0, 30.0, 30.0, 20.0),
                    placed("satır", 140.0, 30.0, 50.0, 20.0),
                ],
                vec![placed("Yan", 400.0, 30.0, 40.0, 20.0)],
            ],
            Script::Spaced,
        );
        assert_eq!(passage.nearest(60.0, 1.0), Some(1));
        assert_eq!(passage.nearest(180.0, 10.0), Some(1));
        assert_eq!(passage.nearest(-20.0, 40.0), Some(2));
        assert_eq!(passage.nearest(135.0, 33.0), Some(4));
        assert_eq!(passage.nearest(280.0, 40.0), Some(5));
        assert_eq!(passage.nearest(320.0, 40.0), Some(6));
        assert_eq!(passage.nearest(20.0, 26.0), Some(2));
        assert_eq!(passage.nearest(20.0, 24.0), Some(0));
        assert_eq!(passage.nearest(20.0, 900.0), Some(2));
        assert_eq!(
            Passage::new(Vec::new(), Script::Spaced).nearest(0.0, 0.0),
            None
        );
    }

    #[test]
    fn a_click_finds_a_word_only_on_it_or_within_reach() {
        let passage = Passage::new(
            vec![vec![
                placed("Kaydet", 0.0, 0.0, 60.0, 20.0),
                placed("kapat", 100.0, 0.0, 50.0, 20.0),
            ]],
            Script::Spaced,
        );
        assert_eq!(passage.under(30.0, 10.0, 4.0), Some(0));
        assert_eq!(passage.under(63.0, 22.0, 4.0), Some(0));
        assert_eq!(passage.under(80.0, 10.0, 4.0), None);
        assert_eq!(passage.under(120.0, 30.0, 4.0), None);
        let tight = Passage::new(
            vec![
                vec![placed("Uzun", 200.0, 0.0, 40.0, 20.0)],
                vec![placed("Kısa", 0.0, 25.0, 40.0, 20.0)],
            ],
            Script::Spaced,
        );
        assert_eq!(tight.nearest(220.0, 23.0), Some(1));
        assert_eq!(tight.under(220.0, 23.0, 4.0), Some(0));
    }

    #[test]
    fn a_right_to_left_line_is_chosen_from_its_right() {
        let passage = Passage::new(
            vec![
                vec![
                    placed("عالم", 0.0, 0.0, 40.0, 20.0),
                    placed("مرحبا", 50.0, 0.0, 50.0, 20.0),
                ],
                vec![
                    placed("بك", 30.0, 30.0, 20.0, 20.0),
                    placed("أهلا", 60.0, 30.0, 40.0, 20.0),
                ],
            ],
            Script::RightToLeft,
        );
        assert_eq!(passage.nearest(90.0, 10.0), Some(0));
        assert_eq!(passage.nearest(10.0, 10.0), Some(1));
        assert_eq!(passage.chosen_text(1..=2, LINES), "عالم\nأهلا");
        assert_eq!(passage.text(JOINED), "مرحبا عالم أهلا بك");
    }

    #[test]
    fn the_words_chosen_give_their_lines_in_reading_order() {
        let passage = Passage::new(
            vec![
                line(&["Koyu", "temayı", "açma"]),
                line(&["Bilgisayar", "iPhone", "ve", "iPad"]),
                line(&["Kullanmak", "istediğiniz"]),
            ],
            Script::Spaced,
        );
        assert_eq!(
            passage.chosen_text(1..=4, LINES),
            "temayı açma\nBilgisayar iPhone"
        );
        assert_eq!(passage.chosen_text(5..=5, LINES), "ve");
        assert_eq!(
            passage.chosen_text(2..=7, JOINED),
            "açma Bilgisayar iPhone ve iPad Kullanmak"
        );
        assert_eq!(passage.chosen_text(20..=30, LINES), "");
    }

    #[test]
    fn joined_lines_are_spaced_unless_both_sides_are_written_without_spaces() {
        let spaced = Passage::new(
            vec![line(&["Chrome'u", "koyu"]), line(&["temada", "kullanın."])],
            Script::Spaced,
        );
        assert_eq!(spaced.text(JOINED), "Chrome'u koyu temada kullanın.");
        let unspaced = Passage::new(
            vec![
                line(&["快", "捷"]),
                line(&["键"]),
                line(&["Version", "2"]),
                line(&["Bediz"]),
            ],
            Script::Unspaced,
        );
        assert_eq!(unspaced.text(JOINED), "快捷键Version 2 Bediz");
        assert_eq!(
            Passage::new(vec![line(&["Bediz"])], Script::Spaced).text(JOINED),
            "Bediz"
        );
    }

    #[test]
    fn indentation_is_kept_in_spaces_as_far_in_as_each_line_starts() {
        let passage = code(&[
            (0, &["fn", "main()", "{"]),
            (4, &["if", "ready", "{"]),
            (8, &["println!(\"ok\");"]),
            (16, &["deep();"]),
            (4, &["}"]),
            (0, &["}"]),
        ]);
        assert_eq!(
            passage.text(INDENTED),
            "fn main() {\n    if ready {\n        println!(\"ok\");\n                deep();\n    }\n}"
        );
        assert_eq!(
            passage.text(LINES),
            "fn main() {\nif ready {\nprintln!(\"ok\");\ndeep();\n}\n}"
        );
        assert_eq!(
            passage.text(Layout {
                join_lines: true,
                keep_indentation: true,
            }),
            "fn main() { if ready { println!(\"ok\"); deep(); } }"
        );
    }

    #[test]
    fn chosen_lines_are_indented_from_the_furthest_out_of_them_and_a_line_begun_midway_is_not() {
        let passage = code(&[
            (0, &["fn", "main()", "{"]),
            (4, &["if", "ready", "{"]),
            (8, &["go();"]),
            (4, &["}"]),
        ]);
        assert_eq!(
            passage.chosen_text(4..=7, INDENTED),
            "ready {\n    go();\n}"
        );
        assert_eq!(
            passage.chosen_text(1..=6, INDENTED),
            "main() {\n    if ready {\n        go();"
        );
    }

    #[test]
    fn right_to_left_lines_are_indented_from_the_right() {
        let passage = Passage::new(
            vec![
                vec![
                    placed("بك", 100.0, 0.0, 18.0, 14.0),
                    placed("أهلا", 130.0, 0.0, 38.0, 14.0),
                ],
                vec![placed("مرحبا", 90.0, 20.0, 48.0, 14.0)],
            ],
            Script::RightToLeft,
        );
        assert_eq!(passage.text(INDENTED), "أهلا بك\n   مرحبا");
    }

    #[test]
    fn words_narrower_than_a_pixel_a_letter_keep_no_indentation() {
        for width in [0.0, 0.01] {
            let passage = Passage::new(
                vec![
                    vec![placed("ab", 0.0, 0.0, width, 10.0)],
                    vec![placed("b", 1_000.0, 20.0, width, 10.0)],
                ],
                Script::Spaced,
            );
            assert_eq!(passage.text(INDENTED), "ab\nb");
        }
    }
}
