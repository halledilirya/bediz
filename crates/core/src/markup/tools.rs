//! The marking-up tools with their keys, and the colours they draw in.

use super::{Mark, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Pen,
    Highlighter,
    Line,
    Arrow,
    Rectangle,
    Ellipse,
    Text,
    Step,
    Redact,
    Blur,
    Pixelate,
}

impl Tool {
    pub const ALL: [Self; 11] = [
        Self::Pen,
        Self::Highlighter,
        Self::Line,
        Self::Arrow,
        Self::Rectangle,
        Self::Ellipse,
        Self::Text,
        Self::Step,
        Self::Redact,
        Self::Blur,
        Self::Pixelate,
    ];

    pub const fn key(self) -> char {
        match self {
            Self::Pen => 'P',
            Self::Highlighter => 'H',
            Self::Line => 'L',
            Self::Arrow => 'A',
            Self::Rectangle => 'B',
            Self::Ellipse => 'E',
            Self::Text => 'T',
            Self::Step => 'N',
            Self::Redact => 'X',
            Self::Blur => 'U',
            Self::Pixelate => 'M',
        }
    }

    pub fn for_key(key: char) -> Option<Self> {
        let key = key.to_ascii_uppercase();
        Self::ALL.into_iter().find(|tool| tool.key() == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colour {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Colour {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn ink(self) -> Self {
        let luminance = 0.2126 * f32::from(self.red)
            + 0.7152 * f32::from(self.green)
            + 0.0722 * f32::from(self.blue);
        if luminance > 150.0 {
            Self::new(0, 0, 0)
        } else {
            Self::new(255, 255, 255)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Swatch {
    #[default]
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Black,
    White,
}

impl Swatch {
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Purple,
        Self::Black,
        Self::White,
    ];

    pub const fn colour(self) -> Colour {
        match self {
            Self::Red => Colour::new(229, 57, 53),
            Self::Orange => Colour::new(251, 140, 0),
            Self::Yellow => Colour::new(253, 216, 53),
            Self::Green => Colour::new(67, 160, 71),
            Self::Blue => Colour::new(30, 136, 229),
            Self::Purple => Colour::new(142, 36, 170),
            Self::Black => Colour::new(0, 0, 0),
            Self::White => Colour::new(255, 255, 255),
        }
    }

    pub fn of(mark: &Mark) -> Option<Self> {
        if matches!(
            mark.shape,
            Shape::Redact(..) | Shape::Blur(..) | Shape::Pixelate(..)
        ) {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|swatch| swatch.colour() == mark.colour)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    fn mark(shape: Shape, colour: Colour) -> Mark {
        Mark {
            id: 1,
            shape,
            colour,
            width: 3.0,
        }
    }

    #[test]
    fn a_mark_shows_the_colour_it_is_drawn_in_and_an_effect_none() {
        let text = Shape::Text {
            at: Point::new(4, 4),
            text: "a".to_owned(),
            extent: None,
        };
        assert_eq!(
            Swatch::of(&mark(text, Swatch::Blue.colour())),
            Some(Swatch::Blue)
        );
        let arrow = Shape::Arrow(Point::new(0, 0), Point::new(9, 9));
        assert_eq!(Swatch::of(&mark(arrow, Colour::new(1, 2, 3))), None);
        for effect in [Shape::Redact, Shape::Blur, Shape::Pixelate] {
            let covered = effect(Point::new(0, 0), Point::new(9, 9));
            assert_eq!(Swatch::of(&mark(covered, Swatch::Red.colour())), None);
        }
    }

    #[test]
    fn every_tool_has_its_own_key_and_is_found_by_it_in_either_case() {
        for tool in Tool::ALL {
            assert_eq!(Tool::for_key(tool.key()), Some(tool));
            assert_eq!(Tool::for_key(tool.key().to_ascii_lowercase()), Some(tool));
        }
        let mut keys: Vec<char> = Tool::ALL.iter().map(|tool| tool.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Tool::ALL.len());
        for taken in ['R', 'C', 'O'] {
            assert_eq!(Tool::for_key(taken), None);
        }
    }

    #[test]
    fn a_dark_colour_takes_white_ink_and_a_light_one_black() {
        assert_eq!(Swatch::Blue.colour().ink(), Colour::new(255, 255, 255));
        assert_eq!(Swatch::Yellow.colour().ink(), Colour::new(0, 0, 0));
        assert_eq!(Swatch::White.colour().ink(), Colour::new(0, 0, 0));
        assert_eq!(Swatch::Black.colour().ink(), Colour::new(255, 255, 255));
    }
}
