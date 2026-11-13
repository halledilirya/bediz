//! The size of the controls bar and where its grip, clock and buttons sit, at any display scale.

use bediz_core::geometry::Rect;

use super::pixels;

pub(super) const MARK: f32 = 10.0;
pub(super) const MARK_GAP: f32 = 6.0;
const HEIGHT: f32 = 40.0;
const BUTTON_HEIGHT: f32 = 28.0;
const ICON_WIDTH: f32 = 32.0;
const GAP: f32 = 8.0;
const GRIP: f32 = 10.0;
const PADDING: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Content {
    Text { width: i32, marked: bool },
    Icon,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) time: Option<Rect>,
    pub(super) buttons: Vec<Rect>,
}

pub(super) fn grip_at(scale: f32, height: i32) -> Rect {
    let gap = pixels(GAP, scale);
    Rect::new(gap, 0, gap + pixels(GRIP, scale), height)
}

pub(super) fn layout(scale: f32, time_text: Option<i32>, slots: &[Content]) -> Layout {
    let height = pixels(HEIGHT, scale);
    let button = pixels(BUTTON_HEIGHT, scale);
    let gap = pixels(GAP, scale);
    let padding = pixels(PADDING, scale);
    let mark = pixels(MARK, scale) + pixels(MARK_GAP, scale);
    let icon = pixels(ICON_WIDTH, scale);
    let top = (height - button) / 2;
    let mut right = grip_at(scale, height).right;
    let time = time_text.map(|text| {
        let rect = Rect::new(right + gap, 0, right + gap + text + gap, height);
        right = rect.right;
        rect
    });
    let buttons = slots
        .iter()
        .map(|&slot| {
            let width = match slot {
                Content::Text { width, marked } => {
                    width + 2 * padding + if marked { mark } else { 0 }
                }
                Content::Icon => icon,
            };
            let rect = Rect::new(right + gap, top, right + gap + width, top + button);
            right = rect.right;
            rect
        })
        .collect();
    Layout {
        width: right + gap,
        height,
        time,
        buttons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(width: i32, marked: bool) -> Content {
        Content::Text { width, marked }
    }

    #[test]
    fn the_controls_sit_in_one_row_sized_to_their_texts() {
        assert_eq!(
            layout(1.0, Some(30), &[text(30, true), text(50, false)]),
            Layout {
                width: 232,
                height: 40,
                time: Some(Rect::new(26, 0, 64, 40)),
                buttons: vec![Rect::new(72, 6, 142, 34), Rect::new(150, 6, 224, 34)],
            }
        );
    }

    #[test]
    fn icon_buttons_are_narrow_and_the_grip_comes_first() {
        assert_eq!(
            layout(
                1.0,
                Some(30),
                &[Content::Icon, Content::Icon, Content::Icon, Content::Icon]
            ),
            Layout {
                width: 232,
                height: 40,
                time: Some(Rect::new(26, 0, 64, 40)),
                buttons: vec![
                    Rect::new(72, 6, 104, 34),
                    Rect::new(112, 6, 144, 34),
                    Rect::new(152, 6, 184, 34),
                    Rect::new(192, 6, 224, 34),
                ],
            }
        );
        assert_eq!(grip_at(1.0, 40), Rect::new(8, 0, 18, 40));
    }

    #[test]
    fn before_the_recording_the_buttons_follow_the_grip_without_a_clock() {
        assert_eq!(
            layout(1.0, None, &[text(40, true), Content::Icon, Content::Icon]),
            Layout {
                width: 194,
                height: 40,
                time: None,
                buttons: vec![
                    Rect::new(26, 6, 106, 34),
                    Rect::new(114, 6, 146, 34),
                    Rect::new(154, 6, 186, 34),
                ],
            }
        );
    }

    #[test]
    fn the_controls_grow_with_the_display_scale() {
        let larger = layout(1.5, Some(45), &[text(45, true), Content::Icon]);
        assert_eq!(larger.height, 60);
        assert_eq!(larger.buttons.first().map(Rect::height), Some(42));
        assert_eq!(larger.buttons.get(1).map(Rect::width), Some(48));
        let smaller = layout(1.0, Some(30), &[text(30, true), Content::Icon]);
        assert!(larger.width > smaller.width * 3 / 2 - 2);
    }
}
