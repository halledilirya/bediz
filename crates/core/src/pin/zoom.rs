//! A pin's zoom and opacity: the size a pin is made at, the steps its wheel and keys move through,
//! and the stop where it fills its monitor.

pub const FULL: u16 = 100;
pub const FAINTEST: u16 = 10;
pub const OPACITIES: [u16; 10] = [100, 90, 80, 70, 60, 50, 40, 30, 20, 10];
pub const ACTUAL: u32 = 1_000_000;
const POINT: u32 = ACTUAL / 100;
const SMALLEST_ZOOM: u32 = 5 * POINT;
const SMALL_BELOW: u32 = 20 * POINT;
const SMALL_STEP: u32 = 5 * POINT;
const STEP: u32 = 10 * POINT;
const LARGEST_ZOOM: u32 = 500 * POINT;
// No pin's window outgrows the largest texture its image could be held in.
const LARGEST_SIDE: u32 = 16_384;
const SHRUNK_SIDE: u32 = 100;
const OPACITY_STEP: u16 = 10;
const WHEEL_NOTCH: i32 = 120;
const DETENT_MS: u32 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    // In millionths, so a picture fitted to its monitor keeps its size to the pixel.
    pub zoom: u32,
    pub opacity: u16,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            zoom: ACTUAL,
            opacity: FULL,
        }
    }
}

impl Look {
    // A pin covers just what was shown: a picture shown smaller than its pixels keeps that size.
    pub fn fitted(image: (u32, u32), shown: (u32, u32)) -> Self {
        let zoom = if fits(image, shown) {
            ACTUAL
        } else {
            settled(most_within(image, shown), image)
        };
        Self {
            zoom: zoom.min(largest_zoom(image)),
            ..Self::default()
        }
    }

    pub fn zoomed(self, steps: i32, fine: bool, image: (u32, u32), monitor: (u32, u32)) -> Self {
        let lowest = smallest_zoom(image).min(self.zoom);
        let highest = largest_zoom(image).max(self.zoom);
        let fit = fit_stop(image, monitor, fine).filter(|fit| *fit <= highest);
        let mut zoom = self.zoom;
        for _ in 0..steps.unsigned_abs() {
            let next = if steps > 0 {
                above(zoom, fine, image, fit, highest)
            } else {
                below(zoom, fine, image, fit, lowest)
            };
            match next {
                Some(next) => zoom = next,
                None => break,
            }
        }
        Self { zoom, ..self }
    }

    // One wheel message can hold several notches; they stop where the pin first fills its monitor.
    pub fn turned(self, steps: i32, fine: bool, image: (u32, u32), monitor: (u32, u32)) -> Self {
        if self.fills(image, monitor) {
            return self.zoomed(steps, fine, image, monitor);
        }
        let mut look = self;
        for _ in 0..steps.unsigned_abs() {
            let next = look.zoomed(steps.signum(), fine, image, monitor);
            if next == look {
                break;
            }
            look = next;
            if look.fills(image, monitor) {
                break;
            }
        }
        look
    }

    pub fn filled(self, image: (u32, u32), monitor: (u32, u32)) -> Self {
        let fit = settled(most_within(image, monitor), image);
        Self {
            zoom: fit.min(largest_zoom(image)),
            ..self
        }
    }

    pub fn fills(self, image: (u32, u32), monitor: (u32, u32)) -> bool {
        shown(image, self.zoom) == shown(image, self.filled(image, monitor).zoom)
    }

    pub fn actual_size(self, image: (u32, u32)) -> Self {
        Self {
            zoom: ACTUAL.min(largest_zoom(image)),
            ..self
        }
    }

    pub fn faded(self, steps: i32) -> Self {
        Self {
            opacity: stepped(self.opacity, steps, FAINTEST, FULL),
            ..self
        }
    }

    pub fn with_opacity(self, opacity: u16) -> Self {
        Self {
            opacity: opacity.clamp(FAINTEST, FULL),
            ..self
        }
    }

    pub fn alpha(self) -> f32 {
        f32::from(self.opacity) / f32::from(FULL)
    }
}

// A fine wheel sends parts of a notch; they add up to a step instead of each making one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wheel {
    carried: i32,
}

impl Wheel {
    pub fn turn(&mut self, delta: i32) -> i32 {
        // A part of a notch the other way is dropped, so a whole notch back always makes a step.
        if self.carried.signum() * delta.signum() < 0 {
            self.carried = 0;
        }
        let total = self.carried.saturating_add(delta);
        let steps = total / WHEEL_NOTCH;
        self.carried = total % WHEEL_NOTCH;
        steps
    }
}

// A wheel turned on without a pause stays at the stop it reached, so one fast turn cannot pass it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Detent {
    last: Option<(u32, bool)>,
    from_stop: bool,
}

impl Detent {
    // A turn that starts at the stop leaves it, or a touchpad's parts of a notch never add up.
    pub fn holds(&mut self, time: u32, upward: bool, at_stop: bool) -> bool {
        let going_on = self
            .last
            .is_some_and(|(last, way)| way == upward && time.wrapping_sub(last) < DETENT_MS);
        if !going_on {
            self.from_stop = at_stop;
        }
        self.last = Some((time, upward));
        at_stop && going_on && !self.from_stop
    }
}

pub fn shown((width, height): (u32, u32), zoom: u32) -> (u32, u32) {
    let side = |length: u32| {
        let full = u64::from(ACTUAL);
        let scaled = (u64::from(length) * u64::from(zoom) + full / 2) / full;
        u32::try_from(scaled.max(1)).unwrap_or(u32::MAX)
    };
    (side(width), side(height))
}

// A large pin shrinks further than a small one, but keeps a hundred pixels along its longer side.
fn smallest_zoom(image: (u32, u32)) -> u32 {
    let mut zoom = SMALL_BELOW;
    while zoom > SMALLEST_ZOOM {
        let lower = zoom - SMALL_STEP;
        let (width, height) = shown(image, lower);
        if width.max(height) < SHRUNK_SIDE {
            break;
        }
        zoom = lower;
    }
    zoom
}

fn largest_zoom(image: (u32, u32)) -> u32 {
    most_within(image, (LARGEST_SIDE, LARGEST_SIDE)).min(LARGEST_ZOOM)
}

// Matches the rounding in `shown`, so the image at this zoom never passes the room by a pixel.
fn most_within((width, height): (u32, u32), (room_width, room_height): (u32, u32)) -> u32 {
    let side = |length: u32, room: u32| {
        let full = u64::from(ACTUAL);
        ((u64::from(room) + 1) * full - full / 2 - 1) / u64::from(length.max(1))
    };
    let zoom = side(width, room_width).min(side(height, room_height));
    u32::try_from(zoom).unwrap_or(u32::MAX)
}

// A zoom showing the pin at the size a step does is that step, so 100 % stays 100 %.
fn settled(zoom: u32, image: (u32, u32)) -> u32 {
    let size = shown(image, zoom);
    [
        step_below(zoom.saturating_add(1), false),
        step_above(zoom.saturating_sub(1), false),
    ]
    .into_iter()
    .flatten()
    .find(|step| shown(image, *step) == size)
    .unwrap_or(zoom)
}

// A fit that a step already shows is left to the step, so the pin rests on a round zoom.
fn fit_stop(image: (u32, u32), monitor: (u32, u32), fine: bool) -> Option<u32> {
    let fit = most_within(image, monitor);
    let size = shown(image, fit);
    let beside = [
        step_below(fit.saturating_add(1), fine),
        step_above(fit.saturating_sub(1), fine),
    ];
    let on_a_step = beside
        .into_iter()
        .flatten()
        .any(|step| shown(image, step) == size);
    (!on_a_step).then_some(fit)
}

fn above(zoom: u32, fine: bool, image: (u32, u32), fit: Option<u32>, highest: u32) -> Option<u32> {
    let now = shown(image, zoom);
    let mut step = step_above(zoom, fine);
    while let Some(same) = step.filter(|step| *step <= highest && shown(image, *step) == now) {
        step = step_above(same, fine);
    }
    let step = step.filter(|step| *step <= highest);
    let fit = fit.filter(|fit| *fit > zoom && shown(image, *fit) != now);
    step.into_iter().chain(fit).min()
}

fn below(zoom: u32, fine: bool, image: (u32, u32), fit: Option<u32>, lowest: u32) -> Option<u32> {
    let now = shown(image, zoom);
    let mut step = step_below(zoom, fine);
    while let Some(same) = step.filter(|step| *step >= lowest && shown(image, *step) == now) {
        step = step_below(same, fine);
    }
    let step = step.filter(|step| *step >= lowest);
    let fit = fit.filter(|fit| *fit < zoom && shown(image, *fit) != now);
    step.into_iter().chain(fit).max()
}

fn step_above(zoom: u32, fine: bool) -> Option<u32> {
    let spacing = if fine {
        POINT
    } else if zoom < SMALL_BELOW {
        SMALL_STEP
    } else {
        STEP
    };
    (zoom / spacing).checked_add(1)?.checked_mul(spacing)
}

fn step_below(zoom: u32, fine: bool) -> Option<u32> {
    let spacing = if fine {
        POINT
    } else if zoom <= SMALL_BELOW {
        SMALL_STEP
    } else {
        STEP
    };
    let step = zoom.checked_sub(1)? / spacing * spacing;
    (step > 0).then_some(step)
}

fn stepped(value: u16, steps: i32, low: u16, high: u16) -> u16 {
    let moved = i32::from(value).saturating_add(steps.saturating_mul(i32::from(OPACITY_STEP)));
    u16::try_from(moved.clamp(i32::from(low), i32::from(high))).unwrap_or(value)
}

fn fits((width, height): (u32, u32), (room_width, room_height): (u32, u32)) -> bool {
    width <= room_width && height <= room_height
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: (u32, u32) = (1920, 1080);
    const PHOTO: (u32, u32) = (3840, 2160);

    fn at(points: u32) -> Look {
        Look {
            zoom: points * POINT,
            ..Look::default()
        }
    }

    #[test]
    fn the_zoom_moves_in_tenths_between_a_fifth_and_five_times_for_a_small_pin() {
        let image = (100, 50);
        let look = Look::default();
        assert_eq!(look.zoomed(1, false, image, ROOM), at(110));
        assert_eq!(look.zoomed(-3, false, image, ROOM), at(70));
        assert_eq!(look.zoomed(-50, false, image, ROOM), at(20));
        assert_eq!(look.zoomed(80, false, image, ROOM), at(500));
        assert_eq!(look.zoomed(i32::MIN, false, image, ROOM), at(20));
        assert_eq!(look.zoomed(i32::MAX, false, image, ROOM), at(500));
        assert_eq!(look.zoomed(0, false, image, ROOM), look);
        let grown = look.zoomed(3, false, image, ROOM);
        assert_eq!(grown.actual_size(image), look);
    }

    #[test]
    fn a_fine_turn_moves_by_one_point_and_settles_on_whole_points() {
        let image = (1000, 500);
        let look = Look::default();
        assert_eq!(look.zoomed(1, true, image, ROOM), at(101));
        assert_eq!(look.zoomed(-15, true, image, ROOM), at(85));
        assert_eq!(at(85).zoomed(1, false, image, ROOM), at(90));
        let odd = Look {
            zoom: 666_796,
            ..Look::default()
        };
        assert_eq!(odd.zoomed(1, true, image, (100_000, 100_000)), at(67));
        assert_eq!(odd.zoomed(-1, true, image, (100_000, 100_000)), at(66));
    }

    #[test]
    fn a_step_that_would_not_change_a_tiny_pin_is_passed_over() {
        let image = (10, 10);
        let look = at(100);
        let next = look.zoomed(1, true, image, ROOM);
        assert_ne!(shown(image, next.zoom), shown(image, look.zoom));
        assert_eq!(next, at(105));
    }

    #[test]
    fn a_large_pin_shrinks_by_twentieths_below_a_fifth_while_a_hundred_pixels_remain() {
        let fifth = at(20);
        let steps: Vec<Look> = (1..=4)
            .map(|steps| fifth.zoomed(-steps, false, PHOTO, ROOM))
            .collect();
        assert_eq!(steps, [at(15), at(10), at(5), at(5)]);
        assert_eq!(
            fifth
                .zoomed(-1, false, PHOTO, ROOM)
                .zoomed(1, false, PHOTO, ROOM),
            fifth
        );
        let smallest = fifth.zoomed(-3, false, PHOTO, ROOM);
        assert_eq!(smallest.zoomed(4, false, PHOTO, ROOM), at(30));
        assert_eq!(smallest_zoom(PHOTO), SMALLEST_ZOOM);
        assert_eq!(smallest_zoom((1536, 864)), 10 * POINT);
        assert_eq!(smallest_zoom((864, 1536)), 10 * POINT);
        assert_eq!(smallest_zoom((500, 200)), 20 * POINT);
        assert_eq!(smallest_zoom((1, 1)), 20 * POINT);
        assert!(shown((1536, 864), 10 * POINT).0 >= SHRUNK_SIDE);
        assert!(shown((1536, 864), 5 * POINT).0 < SHRUNK_SIDE);
    }

    #[test]
    fn a_pin_below_its_least_zoom_never_grows_when_zoomed_out() {
        let image = (500, 200);
        let tiny = at(5);
        assert_eq!(tiny.zoomed(-1, false, image, ROOM), tiny);
        let grown = tiny.zoomed(1, false, image, ROOM);
        assert_eq!(grown, at(10));
        assert_eq!(grown.zoomed(-1, false, image, ROOM), grown);
        let none = Look {
            zoom: 0,
            ..Look::default()
        };
        assert_eq!(none.zoomed(-1, true, (100_000, 100_000), ROOM), none);
    }

    #[test]
    fn a_pin_is_made_just_as_large_as_it_was_shown() {
        let screen = (1920, 1080);
        assert_eq!(Look::fitted(screen, screen), Look::default());
        assert_eq!(Look::fitted((3840, 1080), (3840, 1080)), Look::default());
        assert_eq!(Look::fitted(PHOTO, screen), at(50));
        for room in [(2560, 1440), (1728, 972), (1365, 768)] {
            assert_eq!(shown(PHOTO, Look::fitted(PHOTO, room).zoom), room);
        }
        let tall = (1000, 30_000);
        assert_eq!(shown(tall, Look::fitted(tall, (36, 1080)).zoom), (36, 1080));
        assert_eq!(shown((1, 1), Look::fitted((1, 1), (0, 0)).zoom), (1, 1));
    }

    #[test]
    fn the_wheel_stops_where_the_pin_fills_its_monitor_between_two_steps() {
        let monitor = (2560, 1440);
        let fit = at(60).zoomed(1, false, PHOTO, monitor);
        assert_eq!(shown(PHOTO, fit.zoom), monitor);
        assert!(fit.fills(PHOTO, monitor));
        assert_eq!(fit.zoomed(1, false, PHOTO, monitor), at(70));
        assert_eq!(at(70).zoomed(-1, false, PHOTO, monitor), fit);
        assert_eq!(at(70).zoomed(-2, false, PHOTO, monitor), at(60));
        assert_eq!(at(60).zoomed(7, true, PHOTO, monitor), fit);
        assert_eq!(fit.zoomed(1, true, PHOTO, monitor), at(67));
        assert_eq!(fit.zoomed(-1, true, PHOTO, monitor), at(66));
        let full = (2560, 1440);
        let smaller = Look::default().zoomed(-3, false, full, ROOM);
        assert_eq!(shown(full, smaller.zoom), ROOM);
        assert_eq!(Look::default().zoomed(-4, false, full, ROOM), at(70));
    }

    #[test]
    fn a_fit_that_a_step_already_gives_adds_no_stop_and_never_moves_off_it() {
        assert_eq!(at(40).zoomed(1, false, PHOTO, ROOM), at(50));
        assert_eq!(at(40).zoomed(2, false, PHOTO, ROOM), at(60));
        assert_eq!(at(60).zoomed(-1, false, PHOTO, ROOM), at(50));
        assert_eq!(Look::default().filled(PHOTO, ROOM), at(50));
        assert_eq!(Look::default().filled(ROOM, ROOM), Look::default());
        assert!(at(50).fills(PHOTO, ROOM));
        assert!(!Look::default().fills(PHOTO, ROOM));
    }

    #[test]
    fn filling_keeps_the_opacity_and_shows_the_whole_pin_on_its_monitor() {
        let faded = Look::default().faded(-3);
        let filled = faded.filled(PHOTO, (2560, 1440));
        assert_eq!(filled.opacity, faded.opacity);
        assert_eq!(shown(PHOTO, filled.zoom), (2560, 1440));
        let tall = Look::default().filled((1000, 4000), ROOM);
        assert_eq!(shown((1000, 4000), tall.zoom), (270, 1080));
        let huge = (400_000, 1000);
        let filled = Look::default().filled(huge, ROOM);
        assert!(shown(huge, filled.zoom).0 <= LARGEST_SIDE);
    }

    #[test]
    fn a_pin_fills_its_monitor_even_below_the_wheels_least_zoom_and_comes_back_to_it() {
        let tall = (1000, 30_000);
        let fit = Look::default().filled(tall, ROOM);
        assert_eq!(shown(tall, fit.zoom), (36, 1080));
        assert!(fit.zoom < smallest_zoom(tall));
        assert!(fit.fills(tall, ROOM));
        assert!(!at(5).fills(tall, ROOM));
        assert_eq!(fit.zoomed(-1, false, tall, ROOM), fit);
        assert_eq!(fit.zoomed(1, false, tall, ROOM), at(5));
        assert_eq!(at(5).zoomed(-1, false, tall, ROOM), fit);
        assert_eq!(at(20).zoomed(-4, false, tall, ROOM), fit);
    }

    #[test]
    fn a_pin_grows_past_its_monitor_up_to_five_times_or_the_largest_window() {
        let image = (800, 400);
        let grown = Look::default().zoomed(40, false, image, ROOM);
        assert_eq!(grown, at(500));
        assert!(shown(image, grown.zoom).0 > ROOM.0);
        assert_eq!(grown.zoomed(1, false, image, ROOM), grown);
        let wide = Look::default().zoomed(1, false, (2400, 600), ROOM);
        assert_eq!(wide, at(110));
        let largest = Look::default().zoomed(40, false, PHOTO, ROOM);
        assert_eq!(largest, at(420));
        assert!(shown(PHOTO, largest.zoom).0 <= LARGEST_SIDE);
        assert_eq!(largest.zoomed(1, true, PHOTO, ROOM), at(421));
        assert_eq!(largest.zoomed(10, true, PHOTO, ROOM), at(426));
    }

    #[test]
    fn actual_size_is_a_hundred_unless_the_window_would_be_too_large() {
        assert_eq!(at(37).actual_size(PHOTO), Look::default());
        let long = (20_000, 100);
        let actual = Look::default().actual_size(long);
        assert!(actual.zoom < ACTUAL);
        assert_eq!(shown(long, actual.zoom).0, LARGEST_SIDE);
    }

    #[test]
    fn the_opacity_moves_in_tenths_and_never_hides_the_pin() {
        let look = Look::default();
        assert_eq!(look.faded(-1).opacity, 90);
        assert_eq!(look.faded(-20).opacity, FAINTEST);
        assert_eq!(look.faded(5).opacity, FULL);
        assert_eq!(look.with_opacity(0).opacity, FAINTEST);
        assert_eq!(look.with_opacity(40).opacity, 40);
        assert!((look.faded(-5).alpha() - 0.5).abs() < f32::EPSILON);
        assert!(
            OPACITIES
                .iter()
                .all(|opacity| (FAINTEST..=FULL).contains(opacity))
        );
    }

    #[test]
    fn parts_of_a_notch_add_up_to_one_step_each_way() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.turn(40), 0);
        assert_eq!(wheel.turn(40), 0);
        assert_eq!(wheel.turn(40), 1);
        assert_eq!(wheel.turn(-240), -2);
        assert_eq!(wheel.turn(360), 3);
    }

    #[test]
    fn a_whole_notch_turned_back_after_part_of_one_makes_a_step() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.turn(80), 0);
        assert_eq!(wheel.turn(-120), -1);
        assert_eq!(wheel.turn(-80), 0);
        assert_eq!(wheel.turn(120), 1);
        assert_eq!(wheel.turn(-40), 0);
        assert_eq!(wheel.turn(-80), -1);
    }

    #[test]
    fn a_wheel_turned_on_without_a_pause_stays_at_the_stop() {
        let mut detent = Detent::default();
        assert!(!detent.holds(1_000, true, false));
        assert!(!detent.holds(1_050, true, false));
        assert!(detent.holds(1_100, true, true));
        assert!(detent.holds(1_350, true, true));
        assert!(!detent.holds(1_700, true, true));
        assert!(!detent.holds(1_750, false, true));
        assert!(!detent.holds(u32::MAX - 10, true, false));
        assert!(detent.holds(5, true, true));
        assert!(!Detent::default().holds(5, true, true));
    }

    #[test]
    fn notches_sent_together_stop_where_the_pin_fills_its_monitor() {
        let monitor = (2560, 1440);
        let fit = at(60).zoomed(1, false, PHOTO, monitor);
        assert_eq!(at(60).turned(2, false, PHOTO, monitor), fit);
        assert_eq!(at(70).turned(-2, false, PHOTO, monitor), fit);
        assert_eq!(at(60).turned(10, true, PHOTO, monitor), fit);
        assert_eq!(at(80).turned(-30, true, PHOTO, monitor), fit);
        assert_eq!(fit.turned(2, false, PHOTO, monitor), at(80));
        assert_eq!(fit.turned(-2, false, PHOTO, monitor), at(50));
        assert_eq!(at(40).turned(2, false, PHOTO, ROOM), at(50));
        assert_eq!(at(40).zoomed(2, false, PHOTO, ROOM), at(60));
        assert_eq!(at(70).turned(2, false, PHOTO, monitor), at(90));
        assert_eq!(at(60).turned(0, false, PHOTO, monitor), at(60));
        let small = (100, 50);
        assert_eq!(at(490).turned(5, false, small, ROOM), at(500));
    }

    #[test]
    fn a_fast_turn_held_at_the_stop_stays_there_until_a_pause() {
        let monitor = (2560, 1440);
        let mut detent = Detent::default();
        let mut look = at(60);
        let mut turn = |time, delta: i32, look: &mut Look| {
            if !detent.holds(time, delta > 0, look.fills(PHOTO, monitor)) {
                *look = look.turned(delta / WHEEL_NOTCH, false, PHOTO, monitor);
            }
        };
        turn(1_000, 240, &mut look);
        assert!(look.fills(PHOTO, monitor));
        turn(1_050, 240, &mut look);
        turn(1_100, 120, &mut look);
        assert!(look.fills(PHOTO, monitor));
        turn(1_500, 120, &mut look);
        assert_eq!(look, at(70));
    }

    #[test]
    fn a_touchpad_turn_that_starts_at_the_stop_adds_up_and_leaves_it() {
        let mut detent = Detent::default();
        let mut wheel = Wheel::default();
        let mut look = Look::default();
        let mut steps = 0;
        for event in 0..8 {
            let at_stop = look.fills(ROOM, ROOM);
            assert!(!detent.holds(event * 10, true, at_stop));
            steps += wheel.turn(30);
            look = Look::default().zoomed(steps, false, ROOM, ROOM);
        }
        assert_eq!(look, at(120));
        assert!(!look.fills(ROOM, ROOM));
    }

    #[test]
    fn a_selection_wider_than_the_largest_window_is_pinned_within_it() {
        let wide = (20_000, 1080);
        let made = Look::fitted(wide, wide);
        assert_eq!(made, Look::default().actual_size(wide));
        assert_eq!(shown(wide, made.zoom).0, LARGEST_SIDE);
    }

    #[test]
    fn the_shown_size_rounds_and_never_reaches_nothing() {
        assert_eq!(shown((101, 33), ACTUAL), (101, 33));
        assert_eq!(shown((101, 33), 150 * POINT), (152, 50));
        assert_eq!(shown((3, 1), SMALLEST_ZOOM), (1, 1));
    }
}
