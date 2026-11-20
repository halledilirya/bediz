//! What a screen reader hears while a selection is open: the selection's size whenever it changes,
//! the marking-up tool chosen or put down, and the words chosen among those read.

use bediz_core::markup::Tool;
use bediz_core::ocr::Layout;

use super::Runtime;

const SIZE: &str = "Bediz.Selection";
const TOOL: &str = "Bediz.Tool";
const WORDS: &str = "Bediz.Words";
const SPOKEN: Layout = Layout {
    join_lines: true,
    keep_indentation: false,
};

#[derive(Debug, Default)]
pub(super) struct Heard {
    size: Option<(u32, u32)>,
    tool: Option<Tool>,
    words: Option<(usize, usize)>,
}

#[derive(Debug, PartialEq, Eq)]
struct News {
    size: Option<(u32, u32)>,
    tool: Option<Option<Tool>>,
    words: Option<(usize, usize)>,
}

impl Heard {
    fn news(
        &mut self,
        size: Option<(u32, u32)>,
        tool: Option<Tool>,
        words: Option<(usize, usize)>,
    ) -> News {
        let news = News {
            size: size.filter(|_| size != self.size),
            tool: (tool != self.tool).then_some(tool),
            words: words.filter(|_| words != self.words),
        };
        self.size = size;
        self.tool = tool;
        self.words = words;
        news
    }
}

impl Runtime {
    pub(super) fn announce(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let news = self.heard.news(
            session.settled_size(),
            session.marking().tool(),
            session.chosen_words(),
        );
        let (Some(scene), Some(texts)) = (self.scene.as_ref(), self.menu_texts.as_ref()) else {
            return;
        };
        let size = news
            .size
            .map(|(width, height)| scene.announce(&texts.selection_size_of(width, height), SIZE));
        let tool = news
            .tool
            .map(|tool| scene.announce(texts.markup.tool(tool), TOOL));
        let words = news
            .words
            .and_then(|_| session.words_text(SPOKEN))
            .map(|chosen| scene.announce(&chosen, WORDS));
        let notes = [size, tool, words]
            .into_iter()
            .flatten()
            .filter_map(Result::err)
            .map(|error| format!("a screen reader could not be told of the selection: {error}"))
            .collect();
        self.emit_notes(notes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_size_is_said_once_and_a_move_that_keeps_it_says_nothing() {
        let mut heard = Heard::default();
        assert_eq!(
            heard.news(None, None, None),
            News {
                size: None,
                tool: None,
                words: None,
            }
        );
        let sized = |heard: &mut Heard, size| heard.news(size, None, None).size;
        assert_eq!(sized(&mut heard, Some((800, 600))), Some((800, 600)));
        assert_eq!(sized(&mut heard, Some((800, 600))), None);
        assert_eq!(sized(&mut heard, Some((801, 600))), Some((801, 600)));
        assert_eq!(sized(&mut heard, None), None);
        assert_eq!(sized(&mut heard, Some((801, 600))), Some((801, 600)));
    }

    #[test]
    fn a_tool_is_said_when_chosen_and_again_when_put_down() {
        let mut heard = Heard::default();
        let tool = |heard: &mut Heard, tool| heard.news(None, tool, None).tool;
        assert_eq!(tool(&mut heard, None), None);
        assert_eq!(tool(&mut heard, Some(Tool::Arrow)), Some(Some(Tool::Arrow)));
        assert_eq!(tool(&mut heard, Some(Tool::Arrow)), None);
        assert_eq!(tool(&mut heard, Some(Tool::Pen)), Some(Some(Tool::Pen)));
        assert_eq!(tool(&mut heard, None), Some(None));
    }

    #[test]
    fn the_words_chosen_are_said_when_the_run_changes_and_not_when_it_is_let_go() {
        let mut heard = Heard::default();
        let words = |heard: &mut Heard, chosen| heard.news(None, None, chosen).words;
        assert_eq!(words(&mut heard, Some((2, 5))), Some((2, 5)));
        assert_eq!(words(&mut heard, Some((2, 5))), None);
        assert_eq!(words(&mut heard, Some((0, 9))), Some((0, 9)));
        assert_eq!(words(&mut heard, None), None);
        assert_eq!(words(&mut heard, Some((0, 9))), Some((0, 9)));
    }
}
