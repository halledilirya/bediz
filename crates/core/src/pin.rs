//! Screenshots pinned on top of every window: where a pin's window lies as it is zoomed or its
//! monitor goes, and which pins are shown, hidden or closed.

mod zoom;

pub use zoom::{ACTUAL, Detent, FAINTEST, FULL, Look, OPACITIES, Wheel, shown};

use crate::geometry::{Point, Rect, length};

pub fn border(scale: f32) -> u32 {
    scale.round().max(1.0) as u32
}

pub fn window(at: Point, (width, height): (u32, u32), border: u32) -> Rect {
    let border = length(border);
    Rect::new(
        at.x.saturating_sub(border),
        at.y.saturating_sub(border),
        at.x.saturating_add(length(width)).saturating_add(border),
        at.y.saturating_add(length(height)).saturating_add(border),
    )
}

pub fn image_at(window: Rect, border: u32) -> Point {
    let border = length(border);
    Point::new(
        window.left.saturating_add(border),
        window.top.saturating_add(border),
    )
}

pub fn zoom_centre(at: Point, (width, height): (u32, u32), pointer: Option<Point>) -> Point {
    let image = window(at, (width, height), 0);
    match pointer {
        Some(pointer) if image.contains(pointer) => pointer,
        _ => Point::new(
            at.x.saturating_add(i32::try_from(width / 2).unwrap_or(0)),
            at.y.saturating_add(i32::try_from(height / 2).unwrap_or(0)),
        ),
    }
}

pub fn zoomed_about(at: Point, from: u32, to: u32, about: Point) -> Point {
    let ratio = f64::from(to) / f64::from(from.max(1));
    let along = |start: i32, centre: i32| {
        let moved = f64::from(centre) - (f64::from(centre) - f64::from(start)) * ratio;
        moved
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    };
    Point::new(along(at.x, about.x), along(at.y, about.y))
}

// A pin filling its monitor is moved onto it whole, or the stop would leave it hanging off an edge.
pub fn inside(at: Point, (width, height): (u32, u32), monitor: Rect) -> Point {
    Point::new(
        at.x.min(monitor.right.saturating_sub(length(width)))
            .max(monitor.left),
        at.y.min(monitor.bottom.saturating_sub(length(height)))
            .max(monitor.top),
    )
}

pub fn kept_on(window: Rect, monitors: &[Rect]) -> Rect {
    if monitors.is_empty()
        || monitors
            .iter()
            .any(|monitor| monitor.intersection(&window).is_some())
    {
        return window;
    }
    let centre = |rect: &Rect| {
        (
            (i64::from(rect.left) + i64::from(rect.right)) / 2,
            (i64::from(rect.top) + i64::from(rect.bottom)) / 2,
        )
    };
    let (x, y) = centre(&window);
    let nearest = monitors.iter().min_by_key(|monitor| {
        let (mx, my) = centre(monitor);
        let (across, down) = (mx - x, my - y);
        across
            .saturating_mul(across)
            .saturating_add(down.saturating_mul(down))
    });
    let Some(monitor) = nearest else {
        return window;
    };
    let left = window
        .left
        .min(monitor.right.saturating_sub(length(window.width())))
        .max(monitor.left);
    let top = window
        .top
        .min(monitor.bottom.saturating_sub(length(window.height())))
        .max(monitor.top);
    window.translated(
        left.saturating_sub(window.left),
        top.saturating_sub(window.top),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Show(u64),
    Hide(u64),
    Drop(u64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    pub open: usize,
    pub shown: usize,
    pub restorable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shelf {
    open: Vec<(u64, bool)>,
    closed: Option<u64>,
}

impl Shelf {
    pub fn add(&mut self, id: u64) -> Vec<Change> {
        if self.knows(id) {
            return Vec::new();
        }
        self.open.push((id, true));
        vec![Change::Show(id)]
    }

    // Only the last pin closed can come back, so closing another lets the one before it go.
    pub fn close(&mut self, id: u64) -> Vec<Change> {
        let Some(index) = self.open.iter().position(|(open, _)| *open == id) else {
            return Vec::new();
        };
        let (_, shown) = self.open.remove(index);
        let mut changes = Vec::new();
        if shown {
            changes.push(Change::Hide(id));
        }
        if let Some(older) = self.closed.replace(id) {
            changes.push(Change::Drop(older));
        }
        changes
    }

    pub fn restore(&mut self) -> Vec<Change> {
        let Some(id) = self.closed.take() else {
            return Vec::new();
        };
        self.open.push((id, true));
        vec![Change::Show(id)]
    }

    pub fn show_or_hide(&mut self) -> Vec<Change> {
        let show = self.open.iter().all(|(_, shown)| !shown);
        self.set_shown(show)
    }

    pub fn show_all(&mut self) -> Vec<Change> {
        self.set_shown(true)
    }

    pub fn hide(&mut self, id: u64) -> Vec<Change> {
        match self
            .open
            .iter_mut()
            .find(|(open, shown)| *open == id && *shown)
        {
            Some((_, shown)) => {
                *shown = false;
                vec![Change::Hide(id)]
            }
            None => Vec::new(),
        }
    }

    pub fn hide_all(&mut self) -> Vec<Change> {
        self.set_shown(false)
    }

    pub fn close_all(&mut self) -> Vec<Change> {
        let mut changes: Vec<Change> = self
            .open
            .drain(..)
            .map(|(id, _)| Change::Drop(id))
            .collect();
        changes.extend(self.closed.take().map(Change::Drop));
        changes
    }

    pub fn is_shown(&self, id: u64) -> bool {
        self.open.iter().any(|(open, shown)| *open == id && *shown)
    }

    pub fn shown(&self) -> Vec<u64> {
        self.open
            .iter()
            .filter(|(_, shown)| *shown)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn count(&self) -> Count {
        Count {
            open: self.open.len(),
            shown: self.open.iter().filter(|(_, shown)| *shown).count(),
            restorable: self.closed.is_some(),
        }
    }

    fn knows(&self, id: u64) -> bool {
        self.closed == Some(id) || self.open.iter().any(|(open, _)| *open == id)
    }

    fn set_shown(&mut self, show: bool) -> Vec<Change> {
        self.open
            .iter_mut()
            .filter(|(_, shown)| *shown != show)
            .map(|(id, shown)| {
                *shown = show;
                if show {
                    Change::Show(*id)
                } else {
                    Change::Hide(*id)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    #[test]
    fn the_border_lies_outside_the_image_and_widens_with_the_display_scale() {
        assert_eq!(border(1.0), 1);
        assert_eq!(border(1.25), 1);
        assert_eq!(border(1.5), 2);
        assert_eq!(border(2.0), 2);
        assert_eq!(border(0.5), 1);
        let around = window(Point::new(100, 50), (300, 200), 2);
        assert_eq!(around, Rect::new(98, 48, 402, 252));
        assert_eq!(image_at(around, 2), Point::new(100, 50));
        let widest = window(Point::new(100, 50), (300, 200), u32::MAX);
        assert_eq!(image_at(widest, u32::MAX), Point::new(100, 50));
    }

    #[test]
    fn zooming_keeps_the_pixel_under_the_pointer_where_it_was() {
        let at = Point::new(100, 100);
        let pointer = Point::new(150, 120);
        let moved = zoomed_about(at, ACTUAL, 2 * ACTUAL, pointer);
        assert_eq!(moved, Point::new(50, 80));
        let image_x = |at: Point, zoom: f64| (f64::from(pointer.x - at.x)) / zoom;
        assert!((image_x(at, 1.0) - image_x(moved, 2.0)).abs() < 1.0);
        assert_eq!(zoomed_about(moved, 2 * ACTUAL, ACTUAL, pointer), at);
    }

    #[test]
    fn a_pin_filling_its_monitor_is_moved_onto_it_whole() {
        let monitor = Rect::new(1920, 0, 4480, 1440);
        let size = (2560, 1440);
        assert_eq!(
            inside(Point::new(2100, 300), size, monitor),
            Point::new(1920, 0)
        );
        assert_eq!(
            inside(Point::new(-500, -20), (1000, 500), monitor),
            Point::new(1920, 0)
        );
        assert_eq!(
            inside(Point::new(3000, 200), (1000, 500), monitor),
            Point::new(3000, 200)
        );
        assert_eq!(
            inside(Point::new(4000, 1200), (1000, 500), monitor),
            Point::new(3480, 940)
        );
    }

    #[test]
    fn keys_zoom_about_the_middle_and_the_pointer_counts_only_over_the_image() {
        let at = Point::new(10, 20);
        let size = (200, 100);
        assert_eq!(zoom_centre(at, size, None), Point::new(110, 70));
        assert_eq!(
            zoom_centre(at, size, Some(Point::new(15, 25))),
            Point::new(15, 25)
        );
        assert_eq!(
            zoom_centre(at, size, Some(Point::new(500, 25))),
            Point::new(110, 70)
        );
    }

    #[test]
    fn a_pin_off_every_monitor_comes_onto_the_nearest_and_one_on_a_monitor_stays() {
        let left = Rect::new(0, 0, 1920, 1080);
        let right = Rect::new(1920, 0, 3840, 1080);
        let on_right = Rect::new(2500, 300, 2900, 600);
        assert_eq!(kept_on(on_right, &[left, right]), on_right);
        assert_eq!(kept_on(on_right, &[left]), Rect::new(1520, 300, 1920, 600));
        let partly = Rect::new(1800, 1000, 2000, 1200);
        assert_eq!(kept_on(partly, &[left]), partly);
        let huge = Rect::new(4000, 0, 7000, 2000);
        assert_eq!(kept_on(huge, &[left]), Rect::new(0, 0, 3000, 2000));
        assert_eq!(kept_on(on_right, &[]), on_right);
    }

    #[derive(Clone, Copy, Debug)]
    enum Step {
        Add,
        CloseFirst,
        CloseLast,
        Restore,
        ShowOrHide,
        HideFirst,
        HideAll,
        ShowAll,
        CloseAll,
    }

    const STEPS: [Step; 9] = [
        Step::Add,
        Step::CloseFirst,
        Step::CloseLast,
        Step::Restore,
        Step::ShowOrHide,
        Step::HideFirst,
        Step::HideAll,
        Step::ShowAll,
        Step::CloseAll,
    ];

    #[derive(Default)]
    struct Screen {
        made: u64,
        kept: BTreeMap<u64, bool>,
        gone: BTreeSet<u64>,
    }

    impl Screen {
        fn apply(&mut self, changes: &[Change], order: &[Step]) {
            for change in changes {
                match *change {
                    Change::Show(id) => {
                        assert!(!self.gone.contains(&id), "{order:?}");
                        assert_ne!(self.kept.insert(id, true), Some(true), "{order:?}");
                    }
                    Change::Hide(id) => {
                        assert_eq!(self.kept.insert(id, false), Some(true), "{order:?}");
                    }
                    Change::Drop(id) => {
                        assert!(self.kept.remove(&id).is_some(), "{order:?}");
                        assert!(self.gone.insert(id), "{order:?}");
                    }
                }
            }
        }
    }

    fn run(order: &[Step]) {
        let mut shelf = Shelf::default();
        let mut screen = Screen::default();
        for step in order {
            let first = shelf.open.first().map(|(id, _)| *id);
            let last = shelf.open.last().map(|(id, _)| *id);
            let changes = match step {
                Step::Add => {
                    screen.made += 1;
                    shelf.add(screen.made)
                }
                Step::CloseFirst => first.map(|id| shelf.close(id)).unwrap_or_default(),
                Step::CloseLast => last.map(|id| shelf.close(id)).unwrap_or_default(),
                Step::Restore => shelf.restore(),
                Step::ShowOrHide => shelf.show_or_hide(),
                Step::HideFirst => first.map(|id| shelf.hide(id)).unwrap_or_default(),
                Step::HideAll => shelf.hide_all(),
                Step::ShowAll => shelf.show_all(),
                Step::CloseAll => shelf.close_all(),
            };
            screen.apply(&changes, order);
            let count = shelf.count();
            let shown: Vec<u64> = screen
                .kept
                .iter()
                .filter(|(_, shown)| **shown)
                .map(|(id, _)| *id)
                .collect();
            let mut listed = shelf.shown();
            listed.sort_unstable();
            assert_eq!(listed, shown, "{order:?}");
            assert_eq!(count.shown, shown.len(), "{order:?}");
            assert_eq!(
                count.open + usize::from(count.restorable),
                screen.kept.len(),
                "{order:?}"
            );
            assert!(count.shown <= count.open, "{order:?}");
        }
        screen.apply(&shelf.close_all(), order);
        assert!(screen.kept.is_empty(), "{order:?}");
        assert_eq!(screen.gone.len() as u64, screen.made, "{order:?}");
        assert_eq!(shelf.count(), Count::default(), "{order:?}");
    }

    fn every_order(length: usize, order: &mut Vec<Step>) {
        run(order);
        if order.len() == length {
            return;
        }
        for step in STEPS {
            order.push(step);
            every_order(length, order);
            order.pop();
        }
    }

    #[test]
    fn every_order_of_pinning_closing_restoring_and_hiding_keeps_each_pin_once() {
        every_order(6, &mut Vec::new());
    }

    #[test]
    fn closing_lets_only_the_last_closed_pin_come_back() {
        let mut shelf = Shelf::default();
        shelf.add(1);
        shelf.add(2);
        assert_eq!(shelf.close(1), [Change::Hide(1)]);
        assert_eq!(shelf.close(2), [Change::Hide(2), Change::Drop(1)]);
        assert_eq!(shelf.restore(), [Change::Show(2)]);
        assert_eq!(shelf.restore(), []);
        assert_eq!(shelf.close(7), []);
    }

    #[test]
    fn a_new_pin_leaves_hidden_pins_hidden_and_shows_or_hide_brings_them_back() {
        let mut shelf = Shelf::default();
        shelf.add(1);
        assert_eq!(shelf.hide_all(), [Change::Hide(1)]);
        assert_eq!(shelf.add(2), [Change::Show(2)]);
        assert!(!shelf.is_shown(1));
        assert!(shelf.is_shown(2));
        assert_eq!(shelf.show_or_hide(), [Change::Hide(2)]);
        assert_eq!(shelf.show_or_hide(), [Change::Show(1), Change::Show(2)]);
        assert_eq!(
            shelf.count(),
            Count {
                open: 2,
                shown: 2,
                restorable: false,
            }
        );
    }

    #[test]
    fn a_pin_that_could_not_be_shown_stays_hidden_until_pins_are_shown_again() {
        let mut shelf = Shelf::default();
        shelf.add(1);
        shelf.add(2);
        shelf.add(3);
        shelf.close(3);
        assert_eq!(shelf.hide(2), [Change::Hide(2)]);
        assert_eq!(shelf.hide(2), []);
        assert_eq!(shelf.hide(3), []);
        assert_eq!(shelf.hide(7), []);
        assert_eq!(shelf.shown(), [1]);
        assert_eq!(shelf.show_or_hide(), [Change::Hide(1)]);
        assert_eq!(shelf.show_or_hide(), [Change::Show(1), Change::Show(2)]);
        assert!(shelf.count().restorable);
    }

    #[test]
    fn showing_all_brings_back_the_hidden_pins_beside_one_restored() {
        let mut shelf = Shelf::default();
        shelf.add(1);
        shelf.add(2);
        shelf.hide(1);
        shelf.close(2);
        assert_eq!(shelf.restore(), [Change::Show(2)]);
        assert_eq!(shelf.show_all(), [Change::Show(1)]);
        assert_eq!(shelf.show_all(), []);
        assert_eq!(shelf.count().shown, 2);
    }

    #[test]
    fn closing_all_is_final_and_takes_the_closed_pin_too() {
        let mut shelf = Shelf::default();
        shelf.add(1);
        shelf.add(2);
        shelf.close(1);
        assert_eq!(shelf.close_all(), [Change::Drop(2), Change::Drop(1)]);
        assert_eq!(shelf.restore(), []);
        assert_eq!(shelf.add(2), [Change::Show(2)]);
    }
}
