//! Typing into a text: the caret and the selected part, and the keys that move them, select it
//! all, insert, and erase a character or a word.

use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub caret: usize,
    pub anchor: usize,
}

impl Cursor {
    pub fn at(index: usize) -> Self {
        Self {
            caret: index,
            anchor: index,
        }
    }

    pub fn range(self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    pub fn is_empty(self) -> bool {
        self.caret == self.anchor
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Up,
    Down,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Space,
    Break,
    Word,
    Sign,
}

pub fn placed(text: &str, index: usize) -> Cursor {
    Cursor::at(snapped(text, index))
}

pub fn everything(text: &str) -> Cursor {
    Cursor {
        caret: text.len(),
        anchor: 0,
    }
}

pub fn insert(text: &mut String, cursor: Cursor, typed: &str) -> Cursor {
    let range = clamped(text, cursor.range());
    let start = range.start;
    text.replace_range(range, typed);
    Cursor::at(start + typed.len())
}

pub fn erase(text: &mut String, cursor: Cursor, forward: bool, word: bool) -> Cursor {
    let range = if cursor.is_empty() {
        let caret = snapped(text, cursor.caret);
        match (forward, word) {
            (false, false) => previous(text, caret)..caret,
            (false, true) => word_before(text, caret)..caret,
            (true, false) => caret..next(text, caret),
            (true, true) => caret..word_after(text, caret),
        }
    } else {
        clamped(text, cursor.range())
    };
    let start = range.start;
    text.replace_range(range, "");
    Cursor::at(start)
}

pub fn moved(text: &str, cursor: Cursor, motion: Motion, extend: bool) -> Cursor {
    if !extend && !cursor.is_empty() {
        let range = clamped(text, cursor.range());
        match motion {
            Motion::Left => return Cursor::at(range.start),
            Motion::Right => return Cursor::at(range.end),
            _ => {}
        }
    }
    let caret = snapped(text, cursor.caret);
    let to = match motion {
        Motion::Left => previous(text, caret),
        Motion::Right => next(text, caret),
        Motion::WordLeft => word_before(text, caret),
        Motion::WordRight => word_after(text, caret),
        Motion::LineStart => line_start(text, caret),
        Motion::LineEnd => line_end(text, caret),
        Motion::Up => line_above(text, caret),
        Motion::Down => line_below(text, caret),
        Motion::Start => 0,
        Motion::End => text.len(),
    };
    if extend {
        Cursor {
            caret: to,
            anchor: snapped(text, cursor.anchor),
        }
    } else {
        Cursor::at(to)
    }
}

fn kind(character: char) -> Kind {
    if character == '\n' {
        Kind::Break
    } else if character.is_whitespace() {
        Kind::Space
    } else if character.is_alphanumeric() || character == '_' {
        Kind::Word
    } else {
        Kind::Sign
    }
}

fn snapped(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

fn clamped(text: &str, range: Range<usize>) -> Range<usize> {
    snapped(text, range.start)..snapped(text, range.end)
}

fn previous(text: &str, index: usize) -> usize {
    text.get(..index)
        .and_then(|before| before.char_indices().next_back())
        .map_or(0, |(at, _)| at)
}

fn next(text: &str, index: usize) -> usize {
    text.get(index..)
        .and_then(|after| after.chars().next())
        .map_or(index, |character| index + character.len_utf8())
}

fn word_before(text: &str, index: usize) -> usize {
    let before = text.get(..index).unwrap_or_default();
    let mut characters = before.char_indices().rev().peekable();
    let mut start = index;
    while let Some((at, _)) = characters.next_if(|(_, character)| kind(*character) == Kind::Space) {
        start = at;
    }
    let Some(&(at, character)) = characters.peek() else {
        return start;
    };
    let run = kind(character);
    if run == Kind::Break {
        return if start == index { at } else { start };
    }
    while let Some((at, _)) = characters.next_if(|(_, character)| kind(*character) == run) {
        start = at;
    }
    start
}

fn word_after(text: &str, index: usize) -> usize {
    let after = text.get(index..).unwrap_or_default();
    let mut characters = after.char_indices().peekable();
    let Some(&(_, character)) = characters.peek() else {
        return index;
    };
    let run = kind(character);
    if run == Kind::Break {
        return index + character.len_utf8();
    }
    if run != Kind::Space {
        while characters
            .next_if(|(_, character)| kind(*character) == run)
            .is_some()
        {}
    }
    while characters
        .next_if(|(_, character)| kind(*character) == Kind::Space)
        .is_some()
    {}
    characters.peek().map_or(text.len(), |(at, _)| index + at)
}

fn line_start(text: &str, index: usize) -> usize {
    text.get(..index)
        .and_then(|before| before.rfind('\n'))
        .map_or(0, |at| at + 1)
}

fn line_end(text: &str, index: usize) -> usize {
    text.get(index..)
        .and_then(|after| after.find('\n'))
        .map_or(text.len(), |at| index + at)
}

fn column(text: &str, index: usize) -> usize {
    text.get(line_start(text, index)..index)
        .map_or(0, |part| part.chars().count())
}

fn at_column(text: &str, line: usize, column: usize) -> usize {
    let end = line_end(text, line);
    text.get(line..end)
        .and_then(|part| part.char_indices().nth(column))
        .map_or(end, |(at, _)| line + at)
}

fn line_above(text: &str, index: usize) -> usize {
    let start = line_start(text, index);
    if start == 0 {
        return 0;
    }
    at_column(text, line_start(text, start - 1), column(text, index))
}

fn line_below(text: &str, index: usize) -> usize {
    let end = line_end(text, index);
    if end == text.len() {
        return text.len();
    }
    at_column(text, end + 1, column(text, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str, caret: usize) -> (String, Cursor) {
        (text.to_owned(), Cursor::at(caret))
    }

    #[test]
    fn typing_replaces_the_selected_part() {
        let (mut text, _) = typed("hello world", 0);
        let cursor = insert(
            &mut text,
            Cursor {
                caret: 0,
                anchor: 5,
            },
            "bye",
        );
        assert_eq!(text, "bye world");
        assert_eq!(cursor, Cursor::at(3));
        let all = everything(&text);
        let cursor = insert(&mut text, all, "ş");
        assert_eq!((text.as_str(), cursor), ("ş", Cursor::at(2)));
    }

    #[test]
    fn backspace_and_delete_take_one_character_however_many_bytes_it_has() {
        let (mut text, cursor) = typed("aş😀b", 7);
        let cursor = erase(&mut text, cursor, false, false);
        assert_eq!((text.as_str(), cursor), ("aşb", Cursor::at(3)));
        let cursor = erase(&mut text, Cursor::at(1), true, false);
        assert_eq!((text.as_str(), cursor), ("ab", Cursor::at(1)));
        let cursor = erase(&mut text, Cursor::at(0), false, false);
        assert_eq!((text.as_str(), cursor), ("ab", Cursor::at(0)));
    }

    #[test]
    fn ctrl_backspace_takes_the_word_before_and_ctrl_delete_the_word_after() {
        let (mut text, cursor) = typed("test, presentation  ", 20);
        let cursor = erase(&mut text, cursor, false, true);
        assert_eq!((text.as_str(), cursor), ("test, ", Cursor::at(6)));
        let cursor = erase(&mut text, cursor, false, true);
        assert_eq!((text.as_str(), cursor), ("test", Cursor::at(4)));
        let (mut text, _) = typed("one two three", 0);
        let cursor = erase(&mut text, Cursor::at(4), true, true);
        assert_eq!((text.as_str(), cursor), ("one three", Cursor::at(4)));
    }

    #[test]
    fn ctrl_backspace_at_the_start_of_a_line_takes_only_the_line_break() {
        let (mut text, cursor) = typed("first\nsecond", 6);
        let cursor = erase(&mut text, cursor, false, true);
        assert_eq!((text.as_str(), cursor), ("firstsecond", Cursor::at(5)));
    }

    #[test]
    fn the_caret_moves_by_character_word_and_line() {
        let text = "ab cd\nefgh";
        let at = |index, motion| moved(text, Cursor::at(index), motion, false).caret;
        assert_eq!(at(1, Motion::Left), 0);
        assert_eq!(at(0, Motion::Left), 0);
        assert_eq!(at(1, Motion::Right), 2);
        assert_eq!(at(0, Motion::WordRight), 3);
        assert_eq!(at(5, Motion::WordLeft), 3);
        assert_eq!(at(4, Motion::LineStart), 0);
        assert_eq!(at(1, Motion::LineEnd), 5);
        assert_eq!(at(4, Motion::Down), 10);
        assert_eq!(at(8, Motion::Up), 2);
        assert_eq!(at(2, Motion::Up), 0);
        assert_eq!(at(8, Motion::Down), 10);
        assert_eq!(at(3, Motion::Start), 0);
        assert_eq!(at(3, Motion::End), 10);
        assert_eq!(moved("şş", Cursor::at(4), Motion::Left, false).caret, 2);
    }

    #[test]
    fn shift_extends_the_selection_and_an_arrow_closes_it_at_its_side() {
        let text = "hello world";
        let selected = moved(text, Cursor::at(6), Motion::WordRight, true);
        assert_eq!(selected.range(), 6..11);
        assert_eq!(moved(text, selected, Motion::Left, false), Cursor::at(6));
        assert_eq!(moved(text, selected, Motion::Right, false), Cursor::at(11));
        assert_eq!(everything(text).range(), 0..11);
        assert_eq!(placed("ş", 1), Cursor::at(0));
    }
}
