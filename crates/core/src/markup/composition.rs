//! An input method's composition in a text being typed: the part it has not handed over yet,
//! its caret and how far each clause is converted, shown in place of the selected part.

use std::ops::Range;

use super::typing::Cursor;
use super::{Mark, Shape};

const TARGET_CONVERTED: u8 = 1;
const CONVERTED: u8 = 2;
const TARGET_NOT_CONVERTED: u8 = 3;
const FIXED_CONVERTED: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clause {
    Unconverted,
    Converted,
    Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composition {
    pub text: String,
    pub caret: usize,
    pub clauses: Vec<(Range<usize>, Clause)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Composed {
    pub mark: Mark,
    pub caret: usize,
    pub clauses: Vec<(Range<usize>, Clause)>,
}

impl Composition {
    pub fn from_input_method(
        units: &[u16],
        attributes: &[u8],
        clause_starts: &[u32],
        caret: usize,
    ) -> Option<Self> {
        if units.is_empty() {
            return None;
        }
        let text = String::from_utf16_lossy(units);
        let offsets = byte_offsets(&text);
        let at = |unit: usize| offsets.get(unit).copied().unwrap_or(text.len());
        let changes = (1..attributes.len())
            .filter(|unit| attributes.get(*unit) != attributes.get(unit.saturating_sub(1)));
        let mut cuts: Vec<usize> = clause_starts
            .iter()
            .filter_map(|start| usize::try_from(*start).ok())
            .chain(changes)
            .chain([0, units.len()])
            .filter(|cut| *cut <= units.len())
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        let clauses: Vec<(Range<usize>, Clause)> = cuts
            .windows(2)
            .filter_map(|pair| {
                let &[start, end] = pair else {
                    return None;
                };
                let range = at(start)..at(end);
                (!range.is_empty()).then(|| (range, clause_of(attributes.get(start).copied())))
            })
            .collect();
        // As edit boxes do, the caret follows the clause being converted, not the input method's.
        let caret = clauses
            .iter()
            .rfind(|(_, clause)| *clause == Clause::Target)
            .map_or(at(caret.min(units.len())), |(range, _)| range.end);
        Some(Self {
            caret,
            text,
            clauses,
        })
    }
}

pub fn composed(mark: &Mark, cursor: Cursor, composition: &Composition) -> Option<Composed> {
    let Shape::Text { at, text, .. } = &mark.shape else {
        return None;
    };
    let range = cursor.range();
    let (before, after) = (text.get(..range.start)?, text.get(range.end..)?);
    let start = before.len();
    let mut shown = mark.clone();
    shown.shape = Shape::Text {
        at: *at,
        text: format!("{before}{}{after}", composition.text),
        extent: None,
    };
    let clauses = composition
        .clauses
        .iter()
        .map(|(range, clause)| (start + range.start..start + range.end, *clause))
        .collect();
    Some(Composed {
        mark: shown,
        caret: start + composition.caret,
        clauses,
    })
}

fn clause_of(attribute: Option<u8>) -> Clause {
    match attribute {
        Some(TARGET_CONVERTED | TARGET_NOT_CONVERTED) => Clause::Target,
        Some(CONVERTED | FIXED_CONVERTED) => Clause::Converted,
        _ => Clause::Unconverted,
    }
}

fn byte_offsets(text: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(text.len() + 1);
    for (at, character) in text.char_indices() {
        offsets.extend(std::iter::repeat_n(at, character.len_utf16()));
    }
    offsets.push(text.len());
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;
    use crate::markup::Swatch;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn a_composition_not_yet_converted_is_one_clause_with_its_caret() {
        let typed = units("にほんご");
        let composition = Composition::from_input_method(&typed, &[0; 4], &[0, 4], 4);
        assert_eq!(
            composition,
            Some(Composition {
                text: "にほんご".to_owned(),
                caret: 12,
                clauses: vec![(0..12, Clause::Unconverted)],
            })
        );
    }

    #[test]
    fn clauses_split_where_the_input_method_starts_one_or_the_attribute_changes() {
        let typed = units("日本語を書く");
        let attributes = [1, 1, 1, 2, 2, 2];
        let composition = Composition::from_input_method(&typed, &attributes, &[0, 3, 4, 6], 0);
        let Some(composition) = composition else {
            panic!("a composition with text is kept");
        };
        assert_eq!(
            composition.clauses,
            vec![
                (0..9, Clause::Target),
                (9..12, Clause::Converted),
                (12..18, Clause::Converted),
            ]
        );
    }

    #[test]
    fn the_caret_stands_after_the_clause_being_converted() {
        let typed = units("私は日本語を");
        let attributes = [2, 2, 1, 1, 1, 2];
        let converting = Composition::from_input_method(&typed, &attributes, &[0, 2, 5, 6], 2);
        assert_eq!(converting.map(|composition| composition.caret), Some(15));
        let typed = units("にほんご");
        let typing = Composition::from_input_method(&typed, &[0; 4], &[0, 4], 2);
        assert_eq!(typing.map(|composition| composition.caret), Some(6));
    }

    #[test]
    fn utf16_positions_land_on_whole_characters_and_unknown_attributes_are_unconverted() {
        let typed = units("😀a");
        let composition = Composition::from_input_method(&typed, &[0, 0, 3], &[1, 99], 3);
        let Some(composition) = composition else {
            panic!("a composition with text is kept");
        };
        assert_eq!(composition.caret, 5);
        assert_eq!(
            composition.clauses,
            vec![(0..4, Clause::Unconverted), (4..5, Clause::Target)]
        );
        let bare = Composition::from_input_method(&units("ab"), &[], &[], 99);
        assert_eq!(
            bare.map(|composition| (composition.caret, composition.clauses)),
            Some((2, vec![(0..2, Clause::Unconverted)]))
        );
        assert_eq!(Composition::from_input_method(&[], &[], &[], 0), None);
    }

    #[test]
    fn a_composition_is_shown_in_place_of_the_selected_part() {
        let mark = Mark {
            id: 7,
            shape: Shape::Text {
                at: Point::new(5, 5),
                text: "abcd".to_owned(),
                extent: None,
            },
            colour: Swatch::Red.colour(),
            width: 3.0,
        };
        let composition = Composition {
            text: "にほ".to_owned(),
            caret: 6,
            clauses: vec![(0..6, Clause::Unconverted)],
        };
        let cursor = Cursor {
            caret: 3,
            anchor: 1,
        };
        let Some(composed) = composed(&mark, cursor, &composition) else {
            panic!("a text shows its composition");
        };
        assert_eq!(
            composed.mark.shape,
            Shape::Text {
                at: Point::new(5, 5),
                text: "aにほd".to_owned(),
                extent: None,
            }
        );
        assert_eq!((composed.mark.id, composed.caret), (7, 7));
        assert_eq!(composed.clauses, vec![(1..7, Clause::Unconverted)]);
    }
}
