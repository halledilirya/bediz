//! What a screen reader hears while a selection is open: the selection's size whenever it changes,
//! and the marking-up tool chosen or put down.

use bediz_core::markup::Tool;

use super::Runtime;

const SIZE: &str = "Bediz.Selection";
const TOOL: &str = "Bediz.Tool";

#[derive(Debug, Default)]
pub(super) struct Heard {
    size: Option<(u32, u32)>,
    tool: Option<Tool>,
}

#[derive(Debug, PartialEq, Eq)]
struct News {
    size: Option<(u32, u32)>,
    tool: Option<Option<Tool>>,
}

impl Heard {
    fn news(&mut self, size: Option<(u32, u32)>, tool: Option<Tool>) -> News {
        let news = News {
            size: size.filter(|_| size != self.size),
            tool: (tool != self.tool).then_some(tool),
        };
        self.size = size;
        self.tool = tool;
        news
    }
}

impl Runtime {
    pub(super) fn announce(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let news = self
            .heard
            .news(session.settled_size(), session.marking().tool());
        let (Some(scene), Some(texts)) = (self.scene.as_ref(), self.menu_texts.as_ref()) else {
            return;
        };
        let size = news
            .size
            .map(|(width, height)| scene.announce(&texts.selection_size_of(width, height), SIZE));
        let tool = news
            .tool
            .map(|tool| scene.announce(texts.markup.tool(tool), TOOL));
        let notes = [size, tool]
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
            heard.news(None, None),
            News {
                size: None,
                tool: None
            }
        );
        assert_eq!(heard.news(Some((800, 600)), None).size, Some((800, 600)));
        assert_eq!(heard.news(Some((800, 600)), None).size, None);
        assert_eq!(heard.news(Some((801, 600)), None).size, Some((801, 600)));
        assert_eq!(heard.news(None, None).size, None);
        assert_eq!(heard.news(Some((801, 600)), None).size, Some((801, 600)));
    }

    #[test]
    fn a_tool_is_said_when_chosen_and_again_when_put_down() {
        let mut heard = Heard::default();
        assert_eq!(heard.news(None, None).tool, None);
        assert_eq!(
            heard.news(None, Some(Tool::Arrow)).tool,
            Some(Some(Tool::Arrow))
        );
        assert_eq!(heard.news(None, Some(Tool::Arrow)).tool, None);
        assert_eq!(
            heard.news(None, Some(Tool::Pen)).tool,
            Some(Some(Tool::Pen))
        );
        assert_eq!(heard.news(None, None).tool, Some(None));
    }
}
