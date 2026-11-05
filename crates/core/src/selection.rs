use crate::geometry::{Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Handle(Handle),
    Inside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub square: bool,
    pub centered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axes {
    Both,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gesture {
    Drawing {
        anchor: Point,
        offset: Point,
        axes: Axes,
        start: Rect,
        new: bool,
    },
    Moving {
        grab: Point,
        start: Rect,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    bounds: Rect,
    rect: Option<Rect>,
    gesture: Option<Gesture>,
    pointer: Point,
    moving: Point,
}

impl Selection {
    pub fn start(monitors: &[Rect], at: Point) -> Option<Self> {
        monitor_at(monitors, at)?;
        Some(Self {
            bounds: screen(monitors)?,
            rect: None,
            gesture: Some(Gesture::Drawing {
                anchor: at,
                offset: Point::default(),
                axes: Axes::Both,
                start: Rect::default(),
                new: true,
            }),
            pointer: at,
            moving: at,
        })
    }

    pub fn whole_monitor(monitors: &[Rect], at: Point) -> Option<Self> {
        let monitor = monitor_at(monitors, at)?;
        Some(Self {
            bounds: screen(monitors)?,
            rect: Some(monitor),
            gesture: None,
            pointer: at,
            moving: at,
        })
    }

    pub fn restore(monitors: &[Rect], rect: Rect) -> Option<Self> {
        monitors
            .iter()
            .find_map(|monitor| monitor.intersection(&rect))?;
        let bounds = screen(monitors)?;
        let corner = Point::new(rect.left, rect.top);
        Some(Self {
            bounds,
            rect: rect.intersection(&bounds),
            gesture: None,
            pointer: corner,
            moving: corner,
        })
    }

    pub fn grip_at(&self, at: Point, reach: i32) -> Option<Grip> {
        let rect = self.rect?;
        let (last_x, last_y) = (rect.right - 1, rect.bottom - 1);
        let reach_x = reach.min(third(rect.width())).max(1);
        let reach_y = reach.min(third(rect.height())).max(1);
        let near_x = at.x >= rect.left - reach_x && at.x <= last_x + reach_x;
        let near_y = at.y >= rect.top - reach_y && at.y <= last_y + reach_y;
        if !(near_x && near_y) {
            return None;
        }
        let left = (at.x - rect.left).abs() <= reach_x;
        let right = (at.x - last_x).abs() <= reach_x;
        let top = (at.y - rect.top).abs() <= reach_y;
        let bottom = (at.y - last_y).abs() <= reach_y;
        let handle = match (left, right, top, bottom) {
            (true, _, true, _) => Handle::TopLeft,
            (_, true, true, _) => Handle::TopRight,
            (_, true, _, true) => Handle::BottomRight,
            (true, _, _, true) => Handle::BottomLeft,
            (true, _, _, _) => Handle::Left,
            (_, true, _, _) => Handle::Right,
            (_, _, true, _) => Handle::Top,
            (_, _, _, true) => Handle::Bottom,
            _ => return Some(Grip::Inside),
        };
        Some(Grip::Handle(handle))
    }

    pub fn grab(&mut self, grip: Grip, at: Point) {
        let Some(rect) = self.rect else {
            return;
        };
        self.pointer = at;
        self.gesture = Some(match grip {
            Grip::Inside => Gesture::Moving {
                grab: at,
                start: rect,
            },
            Grip::Handle(handle) => {
                let (anchor, moving, axes) = resize_points(rect, handle);
                let offset = match axes {
                    Axes::Both => Point::new(moving.x - at.x, moving.y - at.y),
                    Axes::Horizontal => Point::new(moving.x - at.x, 0),
                    Axes::Vertical => Point::new(0, moving.y - at.y),
                };
                self.moving = self
                    .bounds
                    .clamp(Point::new(at.x + offset.x, at.y + offset.y));
                Gesture::Drawing {
                    anchor,
                    offset,
                    axes,
                    start: rect,
                    new: false,
                }
            }
        });
    }

    pub fn drag(&mut self, to: Point, modifiers: Modifiers) {
        self.pointer = to;
        self.update(modifiers);
    }

    pub fn change_modifiers(&mut self, modifiers: Modifiers) {
        self.update(modifiers);
    }

    pub fn finish(&mut self, at: Point, modifiers: Modifiers) {
        if self.gesture.is_some() {
            self.drag(at, modifiers);
            self.gesture = None;
        }
    }

    pub fn nudge(&mut self, dx: i32, dy: i32) {
        if let (None, Some(rect)) = (self.gesture, self.rect) {
            self.rect = Some(fit(rect.translated(dx, dy), self.bounds));
        }
    }

    pub fn resize_by(&mut self, dw: i32, dh: i32) {
        if let (None, Some(rect)) = (self.gesture, self.rect) {
            let right = rect
                .right
                .saturating_add(dw)
                .clamp(rect.left.saturating_add(1), self.bounds.right);
            let bottom = rect
                .bottom
                .saturating_add(dh)
                .clamp(rect.top.saturating_add(1), self.bounds.bottom);
            self.rect = Some(Rect::new(rect.left, rect.top, right, bottom));
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.gesture.is_some()
    }

    pub fn focus(&self) -> Option<Point> {
        match self.gesture {
            Some(Gesture::Drawing { .. }) => Some(self.moving),
            _ => None,
        }
    }

    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    fn update(&mut self, modifiers: Modifiers) {
        match self.gesture {
            Some(Gesture::Drawing {
                anchor,
                offset,
                axes,
                start,
                new,
            }) => {
                let pointer = self.bounds.clamp(Point::new(
                    self.pointer.x.saturating_add(offset.x),
                    self.pointer.y.saturating_add(offset.y),
                ));
                let modifiers = Modifiers {
                    square: modifiers.square && axes == Axes::Both,
                    centered: modifiers.centered && new,
                };
                let drawn = drawn(self.bounds, anchor, pointer, axes, start, modifiers);
                self.moving = drawn.map_or(pointer, |(_, moving)| moving);
                self.rect = match drawn {
                    Some((rect, _)) => Some(rect),
                    None if new => None,
                    None => self.rect,
                };
            }
            Some(Gesture::Moving { grab, start }) => {
                let moved = start.translated(
                    self.pointer.x.saturating_sub(grab.x),
                    self.pointer.y.saturating_sub(grab.y),
                );
                self.rect = Some(fit(moved, self.bounds));
            }
            None => {}
        }
    }
}

fn monitor_at(monitors: &[Rect], at: Point) -> Option<Rect> {
    monitors.iter().copied().find(|bounds| bounds.contains(at))
}

fn screen(monitors: &[Rect]) -> Option<Rect> {
    monitors
        .iter()
        .copied()
        .reduce(|screen, monitor| screen.union(&monitor))
}

fn third(length: u32) -> i32 {
    i32::try_from(length / 3).unwrap_or(i32::MAX)
}

fn resize_points(rect: Rect, handle: Handle) -> (Point, Point, Axes) {
    let (left, top) = (rect.left, rect.top);
    let (right, bottom) = (rect.right - 1, rect.bottom - 1);
    match handle {
        Handle::TopLeft => (Point::new(right, bottom), Point::new(left, top), Axes::Both),
        Handle::TopRight => (Point::new(left, bottom), Point::new(right, top), Axes::Both),
        Handle::BottomRight => (Point::new(left, top), Point::new(right, bottom), Axes::Both),
        Handle::BottomLeft => (Point::new(right, top), Point::new(left, bottom), Axes::Both),
        Handle::Left => (
            Point::new(right, top),
            Point::new(left, top),
            Axes::Horizontal,
        ),
        Handle::Right => (
            Point::new(left, top),
            Point::new(right, top),
            Axes::Horizontal,
        ),
        Handle::Top => (
            Point::new(left, bottom),
            Point::new(left, top),
            Axes::Vertical,
        ),
        Handle::Bottom => (
            Point::new(left, top),
            Point::new(left, bottom),
            Axes::Vertical,
        ),
    }
}

fn drawn(
    bounds: Rect,
    anchor: Point,
    pointer: Point,
    axes: Axes,
    start: Rect,
    modifiers: Modifiers,
) -> Option<(Rect, Point)> {
    let mut dx = if axes == Axes::Vertical {
        0
    } else {
        pointer.x - anchor.x
    };
    let mut dy = if axes == Axes::Horizontal {
        0
    } else {
        pointer.y - anchor.y
    };
    if modifiers.centered {
        let room_x = (anchor.x - bounds.left).min(bounds.right - 1 - anchor.x);
        let room_y = (anchor.y - bounds.top).min(bounds.bottom - 1 - anchor.y);
        let (mut half_x, mut half_y) = (dx.abs().min(room_x), dy.abs().min(room_y));
        if modifiers.square {
            let side = half_x.max(half_y).min(room_x).min(room_y);
            (half_x, half_y) = (side, side);
        }
        if half_x == 0 && half_y == 0 {
            return None;
        }
        let corner = Point::new(
            if dx < 0 {
                anchor.x - half_x
            } else {
                anchor.x + half_x
            },
            if dy < 0 {
                anchor.y - half_y
            } else {
                anchor.y + half_y
            },
        );
        let rect = Rect::spanning(
            Point::new(anchor.x - half_x, anchor.y - half_y),
            Point::new(anchor.x + half_x, anchor.y + half_y),
        );
        return Some((rect, corner));
    }
    if modifiers.square {
        let room_x = if dx < 0 {
            anchor.x - bounds.left
        } else {
            bounds.right - 1 - anchor.x
        };
        let room_y = if dy < 0 {
            anchor.y - bounds.top
        } else {
            bounds.bottom - 1 - anchor.y
        };
        let side = dx.abs().max(dy.abs()).min(room_x).min(room_y);
        dx = if dx < 0 { -side } else { side };
        dy = if dy < 0 { -side } else { side };
    }
    if dx == 0 && dy == 0 && axes == Axes::Both && start.is_empty() {
        return None;
    }
    let corner = Point::new(anchor.x + dx, anchor.y + dy);
    let rect = Rect::spanning(anchor, corner);
    Some(match axes {
        Axes::Both => (rect, corner),
        Axes::Horizontal => (
            Rect::new(rect.left, start.top, rect.right, start.bottom),
            Point::new(corner.x, pointer.y),
        ),
        Axes::Vertical => (
            Rect::new(start.left, rect.top, start.right, rect.bottom),
            Point::new(pointer.x, corner.y),
        ),
    })
}

fn fit(rect: Rect, bounds: Rect) -> Rect {
    let dx = if rect.left < bounds.left {
        bounds.left - rect.left
    } else if rect.right > bounds.right {
        bounds.right - rect.right
    } else {
        0
    };
    let dy = if rect.top < bounds.top {
        bounds.top - rect.top
    } else if rect.bottom > bounds.bottom {
        bounds.bottom - rect.bottom
    } else {
        0
    };
    rect.translated(dx, dy)
        .intersection(&bounds)
        .unwrap_or(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Modifiers = Modifiers {
        square: false,
        centered: false,
    };
    const SQUARE: Modifiers = Modifiers {
        square: true,
        centered: false,
    };
    const CENTERED: Modifiers = Modifiers {
        square: false,
        centered: true,
    };

    fn monitors() -> [Rect; 2] {
        [Rect::new(0, 0, 3840, 2160), Rect::new(-1920, 200, 0, 1280)]
    }

    fn drawn_from(from: Point, to: Point, modifiers: Modifiers) -> Selection {
        let Some(mut selection) = Selection::start(&monitors(), from) else {
            panic!("selection did not start");
        };
        selection.finish(to, modifiers);
        selection
    }

    fn selected(rect: Rect) -> Selection {
        let Some(selection) = Selection::restore(&monitors(), rect) else {
            panic!("rect is on no monitor");
        };
        selection
    }

    #[test]
    fn a_selection_spans_monitors() {
        let selection = drawn_from(Point::new(-100, 500), Point::new(100, 600), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(-100, 500, 101, 601)));
    }

    #[test]
    fn does_not_start_outside_every_monitor() {
        assert_eq!(Selection::start(&monitors(), Point::new(-100, 0)), None);
        assert_eq!(Selection::start(&[], Point::new(0, 0)), None);
    }

    #[test]
    fn has_no_rect_until_the_cursor_moves() {
        let selection = drawn_from(Point::new(10, 10), Point::new(10, 10), PLAIN);
        assert_eq!(selection.rect(), None);
    }

    #[test]
    fn drag_is_clamped_to_the_screen() {
        let Some(mut selection) = Selection::start(&monitors(), Point::new(100, 100)) else {
            panic!("selection did not start");
        };
        selection.drag(Point::new(-5000, 3000), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(-1920, 100, 101, 2160)));
    }

    #[test]
    fn finish_stops_the_drag_and_keeps_the_rect() {
        let mut selection = drawn_from(Point::new(-1000, 300), Point::new(-800, 500), PLAIN);
        assert!(!selection.is_dragging());
        let rect = Some(Rect::new(-1000, 300, -799, 501));
        assert_eq!(selection.rect(), rect);
        selection.drag(Point::new(-1500, 900), PLAIN);
        assert_eq!(selection.rect(), rect);
    }

    #[test]
    fn shift_draws_a_square_in_the_drag_direction() {
        let selection = drawn_from(Point::new(100, 100), Point::new(130, 110), SQUARE);
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 131, 131)));
        let selection = drawn_from(Point::new(100, 100), Point::new(90, 60), SQUARE);
        assert_eq!(selection.rect(), Some(Rect::new(60, 60, 101, 101)));
    }

    #[test]
    fn a_square_stops_at_the_screen_edge() {
        let selection = drawn_from(Point::new(3830, 100), Point::new(3900, 200), SQUARE);
        assert_eq!(selection.rect(), Some(Rect::new(3830, 100, 3840, 110)));
    }

    #[test]
    fn alt_grows_from_the_center() {
        let selection = drawn_from(Point::new(100, 100), Point::new(110, 105), CENTERED);
        assert_eq!(selection.rect(), Some(Rect::new(90, 95, 111, 106)));
    }

    #[test]
    fn a_centered_selection_stays_on_the_screen() {
        let selection = drawn_from(Point::new(3835, 100), Point::new(3900, 120), CENTERED);
        assert_eq!(selection.rect(), Some(Rect::new(3831, 80, 3840, 121)));
    }

    #[test]
    fn shift_and_alt_together_draw_a_centered_square() {
        let both = Modifiers {
            square: true,
            centered: true,
        };
        let selection = drawn_from(Point::new(100, 100), Point::new(104, 110), both);
        assert_eq!(selection.rect(), Some(Rect::new(90, 90, 111, 111)));
    }

    #[test]
    fn grips_are_found_at_corners_edges_and_inside() {
        let selection = selected(Rect::new(100, 100, 201, 151));
        let grip = |x, y| selection.grip_at(Point::new(x, y), 4);
        assert_eq!(grip(100, 100), Some(Grip::Handle(Handle::TopLeft)));
        assert_eq!(grip(203, 98), Some(Grip::Handle(Handle::TopRight)));
        assert_eq!(grip(200, 150), Some(Grip::Handle(Handle::BottomRight)));
        assert_eq!(grip(97, 152), Some(Grip::Handle(Handle::BottomLeft)));
        assert_eq!(grip(99, 125), Some(Grip::Handle(Handle::Left)));
        assert_eq!(grip(202, 125), Some(Grip::Handle(Handle::Right)));
        assert_eq!(grip(150, 101), Some(Grip::Handle(Handle::Top)));
        assert_eq!(grip(150, 153), Some(Grip::Handle(Handle::Bottom)));
        assert_eq!(grip(150, 125), Some(Grip::Inside));
        assert_eq!(grip(150, 160), None);
        assert_eq!(grip(90, 125), None);
    }

    #[test]
    fn a_tiny_selection_keeps_its_inside_reachable() {
        let selection = selected(Rect::new(100, 100, 110, 110));
        assert_eq!(
            selection.grip_at(Point::new(105, 105), 8),
            Some(Grip::Inside)
        );
    }

    #[test]
    fn no_grip_without_a_rect() {
        let Some(selection) = Selection::start(&monitors(), Point::new(10, 10)) else {
            panic!("selection did not start");
        };
        assert_eq!(selection.grip_at(Point::new(10, 10), 4), None);
    }

    #[test]
    fn moving_keeps_the_size_and_stays_on_the_screen() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Inside, Point::new(150, 120));
        selection.drag(Point::new(170, 90), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(120, 70, 221, 121)));
        selection.finish(Point::new(-400, -400), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(-450, 0, -349, 51)));
    }

    #[test]
    fn an_edge_follows_the_pointer_from_where_it_was_grabbed() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Handle(Handle::Right), Point::new(203, 120));
        selection.drag(Point::new(213, 140), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 211, 151)));
    }

    #[test]
    fn the_focus_is_the_pixel_being_moved_not_the_pointer() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        assert_eq!(selection.focus(), None);
        selection.grab(Grip::Handle(Handle::Right), Point::new(203, 120));
        assert_eq!(selection.focus(), Some(Point::new(200, 120)));
        selection.drag(Point::new(213, 140), PLAIN);
        assert_eq!(selection.focus(), Some(Point::new(210, 140)));
        selection.finish(Point::new(213, 140), PLAIN);
        assert_eq!(selection.focus(), None);
    }

    #[test]
    fn a_new_selection_focuses_the_pointer_kept_on_the_screen() {
        let Some(mut selection) = Selection::start(&monitors(), Point::new(100, 100)) else {
            panic!("selection did not start");
        };
        selection.drag(Point::new(4000, 50), PLAIN);
        assert_eq!(selection.focus(), Some(Point::new(3839, 50)));
    }

    #[test]
    fn the_focus_is_the_corner_that_shift_and_alt_place() {
        let start = |at| match Selection::start(&monitors(), at) {
            Some(selection) => selection,
            None => panic!("selection did not start"),
        };
        let mut square = start(Point::new(100, 100));
        square.drag(Point::new(130, 110), SQUARE);
        assert_eq!(square.focus(), Some(Point::new(130, 130)));
        square.drag(Point::new(90, 60), SQUARE);
        assert_eq!(square.focus(), Some(Point::new(60, 60)));
        let mut centered = start(Point::new(3835, 100));
        centered.drag(Point::new(3700, 120), CENTERED);
        assert_eq!(centered.focus(), Some(Point::new(3831, 120)));
        let mut both = start(Point::new(100, 100));
        both.drag(
            Point::new(104, 110),
            Modifiers {
                square: true,
                centered: true,
            },
        );
        assert_eq!(both.focus(), Some(Point::new(110, 110)));
    }

    #[test]
    fn a_squared_corner_resize_focuses_the_squared_corner() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Handle(Handle::BottomRight), Point::new(200, 150));
        selection.drag(Point::new(220, 160), SQUARE);
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 221, 221)));
        assert_eq!(selection.focus(), Some(Point::new(220, 220)));
    }

    #[test]
    fn moving_has_no_focus() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Inside, Point::new(150, 120));
        assert_eq!(selection.focus(), None);
    }

    #[test]
    fn dragging_an_edge_past_the_other_one_flips_the_selection() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Handle(Handle::Left), Point::new(100, 120));
        selection.finish(Point::new(250, 120), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(200, 100, 251, 151)));
    }

    #[test]
    fn a_corner_resizes_both_sides_and_shift_squares_it() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Handle(Handle::BottomRight), Point::new(200, 150));
        selection.drag(Point::new(220, 160), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 221, 161)));
        selection.change_modifiers(SQUARE);
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 221, 221)));
    }

    #[test]
    fn alt_does_not_center_a_resize() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Handle(Handle::BottomRight), Point::new(200, 150));
        selection.finish(Point::new(220, 160), CENTERED);
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 221, 161)));
    }

    #[test]
    fn a_resize_down_to_one_pixel_keeps_the_selection() {
        let mut selection = selected(Rect::new(100, 100, 201, 151));
        selection.grab(Grip::Handle(Handle::BottomRight), Point::new(200, 150));
        selection.finish(Point::new(100, 100), PLAIN);
        assert_eq!(selection.rect(), Some(Rect::new(100, 100, 101, 101)));
    }

    #[test]
    fn nudging_moves_and_stops_at_the_screen_edge() {
        let mut selection = selected(Rect::new(3790, 2110, 3835, 2155));
        selection.nudge(1, 0);
        assert_eq!(selection.rect(), Some(Rect::new(3791, 2110, 3836, 2155)));
        selection.nudge(10, 10);
        assert_eq!(selection.rect(), Some(Rect::new(3795, 2115, 3840, 2160)));
    }

    #[test]
    fn resizing_by_keys_keeps_one_pixel_and_the_monitor() {
        let mut selection = selected(Rect::new(3800, 2100, 3830, 2150));
        selection.resize_by(10, 10);
        assert_eq!(selection.rect(), Some(Rect::new(3800, 2100, 3840, 2160)));
        selection.resize_by(-100, -100);
        assert_eq!(selection.rect(), Some(Rect::new(3800, 2100, 3801, 2101)));
    }

    #[test]
    fn keys_do_nothing_while_dragging() {
        let Some(mut selection) = Selection::start(&monitors(), Point::new(10, 10)) else {
            panic!("selection did not start");
        };
        selection.drag(Point::new(20, 20), PLAIN);
        selection.nudge(5, 5);
        selection.resize_by(5, 5);
        assert_eq!(selection.rect(), Some(Rect::new(10, 10, 21, 21)));
    }

    #[test]
    fn whole_monitor_selects_the_monitor_under_the_cursor() {
        let selection = Selection::whole_monitor(&monitors(), Point::new(-5, 300));
        assert_eq!(selection.and_then(|s| s.rect()), Some(monitors()[1]));
        assert_eq!(
            Selection::whole_monitor(&monitors(), Point::new(-5, 0)),
            None
        );
    }

    #[test]
    fn a_window_across_two_monitors_is_selected_whole() {
        let window = Rect::new(-500, 300, 700, 900);
        let selection = Selection::restore(&monitors(), window);
        assert_eq!(selection.and_then(|s| s.rect()), Some(window));
    }

    #[test]
    fn a_restored_region_is_cut_to_the_screen() {
        let selection = Selection::restore(&monitors(), Rect::new(3700, 2000, 4000, 2300));
        assert_eq!(
            selection.and_then(|s| s.rect()),
            Some(Rect::new(3700, 2000, 3840, 2160))
        );
        let partly_in_a_gap = Rect::new(-5, 0, 10, 10);
        assert_eq!(
            Selection::restore(&monitors(), partly_in_a_gap).and_then(|s| s.rect()),
            Some(partly_in_a_gap)
        );
    }

    #[test]
    fn a_region_only_in_a_gap_selects_nothing() {
        assert_eq!(
            Selection::restore(&monitors(), Rect::new(-1900, 0, -1000, 150)),
            None
        );
    }
}
