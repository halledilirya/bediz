//! The size of the controls bar and where its grip, clock and buttons sit, at any display scale,
//! in as many rows as it takes to fit the width of its monitor.

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

pub(super) fn layout(scale: f32, time_text: Option<i32>, slots: &[Content], room: i32) -> Layout {
    let row = pixels(HEIGHT, scale);
    let button = pixels(BUTTON_HEIGHT, scale);
    let gap = pixels(GAP, scale);
    let padding = pixels(PADDING, scale);
    let mark = pixels(MARK, scale) + pixels(MARK_GAP, scale);
    let icon = pixels(ICON_WIDTH, scale);
    let edge = (row - button) / 2;
    let widths: Vec<i32> = slots
        .iter()
        .map(|&slot| match slot {
            Content::Text { width, marked } => width + 2 * padding + if marked { mark } else { 0 },
            Content::Icon => icon,
        })
        .collect();
    let grip = grip_at(scale, row).right;
    let start = time_text.map_or(grip, |text| grip + gap + text + gap);
    let per_row = per_row(&widths, start, gap, room);
    let rows = widths.len().div_ceil(per_row).max(1) as i32;
    let height = row + (rows - 1) * (button + edge);
    let mut buttons = Vec::with_capacity(widths.len());
    let mut widest = start;
    for (index, line) in widths.chunks(per_row).enumerate() {
        let top = edge + index as i32 * (button + edge);
        let mut right = start;
        for width in line {
            let rect = Rect::new(right + gap, top, right + gap + width, top + button);
            right = rect.right;
            buttons.push(rect);
        }
        widest = widest.max(right);
    }
    Layout {
        width: widest + gap,
        height,
        time: time_text.map(|_| Rect::new(grip + gap, 0, start, height)),
        buttons,
    }
}

fn per_row(widths: &[i32], start: i32, gap: i32, room: i32) -> usize {
    let fits = |per_row: usize| {
        widths
            .chunks(per_row)
            .all(|line| start + line.iter().map(|width| gap + width).sum::<i32>() + gap <= room)
    };
    let count = widths.len().max(1);
    (1..=count)
        .map(|rows| count.div_ceil(rows))
        .find(|&per_row| fits(per_row))
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: i32 = 10_000;

    fn text(width: i32, marked: bool) -> Content {
        Content::Text { width, marked }
    }

    #[test]
    fn the_controls_sit_in_one_row_sized_to_their_texts() {
        assert_eq!(
            layout(1.0, Some(30), &[text(30, true), text(50, false)], WIDE),
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
                &[Content::Icon, Content::Icon, Content::Icon, Content::Icon],
                WIDE
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
            layout(
                1.0,
                None,
                &[text(40, true), Content::Icon, Content::Icon],
                WIDE
            ),
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
        let larger = layout(1.5, Some(45), &[text(45, true), Content::Icon], WIDE);
        assert_eq!(larger.height, 60);
        assert_eq!(larger.buttons.first().map(Rect::height), Some(42));
        assert_eq!(larger.buttons.get(1).map(Rect::width), Some(48));
        let smaller = layout(1.0, Some(30), &[text(30, true), Content::Icon], WIDE);
        assert!(larger.width > smaller.width * 3 / 2 - 2);
    }

    #[test]
    fn a_bar_too_wide_for_its_monitor_wraps_into_even_rows() {
        let icons = [Content::Icon; 17];
        let one_row = layout(2.0, None, &icons, WIDE);
        assert_eq!(one_row.width, 1412);
        let wrapped = layout(2.0, None, &icons, 1366);
        assert_eq!(wrapped.width, 772);
        assert_eq!(wrapped.height, 148);
        let tops: Vec<i32> = wrapped.buttons.iter().map(|button| button.top).collect();
        assert_eq!(tops.iter().filter(|&&top| top == 12).count(), 9);
        assert_eq!(tops.iter().filter(|&&top| top == 80).count(), 8);
    }

    #[test]
    fn every_button_stays_on_the_bar_and_the_bar_within_its_monitor() {
        let icons = [Content::Icon; 17];
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 3.5] {
            for room in [800, 1080, 1280, 1366, 1440, 1920, 2560] {
                let laid = layout(scale, None, &icons, room);
                assert!(laid.width <= room, "{scale} {room}");
                assert_eq!(laid.buttons.len(), icons.len());
                let bar = Rect::new(0, 0, laid.width, laid.height);
                for (index, button) in laid.buttons.iter().enumerate() {
                    assert!(
                        bar.intersection(button) == Some(*button),
                        "{scale} {room} {index}"
                    );
                    for other in laid.buttons.iter().skip(index + 1) {
                        assert!(button.intersection(other).is_none(), "{scale} {room}");
                    }
                }
            }
        }
    }
}
