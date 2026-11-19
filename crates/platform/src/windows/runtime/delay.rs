//! The screenshot taken a few seconds after the tray menu asks for it, with the seconds left shown
//! in a corner of the monitor under the pointer.

use std::time::Duration;

use bediz_core::countdown;
use bediz_core::geometry::Rect;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::screenshot::pointer_rect;
use super::{DELAY_TIMER, Runtime};
use crate::event::Event;
use crate::windows::clock::Tick;
use crate::windows::countdown::Countdown;
use crate::windows::recording::monitor_metrics;
use crate::windows::session::Purpose;

pub struct Delay {
    started: Tick,
    length: Duration,
    countdown: Option<Countdown>,
}

trait Delays {
    fn may_start(&self) -> bool;
    fn begin(&mut self, length: Duration);
    fn left(&self) -> Option<Duration>;
    fn show(&mut self, number: u64);
    fn wake_after(&mut self, after: Duration) -> bool;
    fn end(&mut self);
    fn tell(&mut self, pending: bool);
    fn take_screenshot(&mut self);
    fn timer_failed(&mut self);
    fn note(&mut self, note: &str);
}

impl Runtime {
    pub(super) fn capture_in(&mut self, length: Duration) {
        start(self, length);
    }

    pub(super) fn tick_delay(&mut self) {
        tick(self);
    }

    pub(super) fn cancel_delay(&mut self) {
        cancel(self);
    }

    pub(super) fn counts_down_in(&self, window: HWND) -> bool {
        self.delay
            .as_ref()
            .and_then(|delay| delay.countdown.as_ref())
            .is_some_and(|countdown| countdown.is(window))
    }
}

fn start(delays: &mut impl Delays, length: Duration) {
    cancel(delays);
    if !delays.may_start() {
        delays.note("a delayed screenshot was asked while the overlay could not open");
        return;
    }
    delays.begin(length);
    delays.tell(true);
    tick(delays);
}

fn tick(delays: &mut impl Delays) {
    let Some(left) = delays.left() else {
        return;
    };
    if left.is_zero() {
        cancel(delays);
        // Every opening and quit cancels a delay; this catches a path that does not.
        if delays.may_start() {
            delays.take_screenshot();
        } else {
            delays.note("a delayed screenshot ran out while the overlay could not open");
        }
        return;
    }
    delays.show(countdown::number(left));
    if !delays.wake_after(countdown::until_change(left)) {
        cancel(delays);
        delays.timer_failed();
    }
}

fn cancel(delays: &mut impl Delays) {
    if delays.left().is_some() {
        delays.end();
        delays.tell(false);
    }
}

impl Delays for Runtime {
    fn may_start(&self) -> bool {
        self.can_open() && !self.menu_capture && !self.quitting && self.quit.is_none()
    }

    fn begin(&mut self, length: Duration) {
        let (_, work) = monitor_metrics(pointer_rect());
        let corner = Rect::new(
            work.right.saturating_sub(1),
            work.bottom.saturating_sub(1),
            work.right,
            work.bottom,
        );
        let countdown = match Countdown::in_corner(self.instance, corner, self.colours, length) {
            Ok(countdown) => Some(countdown),
            Err(error) => {
                self.emit_notes(vec![format!(
                    "the seconds left before the screenshot could not be shown: {error}"
                )]);
                None
            }
        };
        self.delay = Some(Delay {
            started: self.clock.now(),
            length,
            countdown,
        });
    }

    fn left(&self) -> Option<Duration> {
        self.delay
            .as_ref()
            .map(|delay| delay.length.saturating_sub(self.clock.since(delay.started)))
    }

    fn show(&mut self, number: u64) {
        let shown = self
            .delay
            .as_mut()
            .and_then(|delay| delay.countdown.as_mut())
            .map(|countdown| countdown.show(number));
        if let Some(Err(error)) = shown {
            self.emit_notes(vec![format!(
                "the seconds left before the screenshot could not be updated: {error}"
            )]);
        }
    }

    fn wake_after(&mut self, after: Duration) -> bool {
        let milliseconds = u32::try_from(after.as_millis()).unwrap_or(u32::MAX).max(1);
        let timer = unsafe { SetTimer(Some(self.window.0), DELAY_TIMER, milliseconds, None) };
        timer != 0
    }

    fn end(&mut self) {
        self.delay = None;
        let _ = unsafe { KillTimer(Some(self.window.0), DELAY_TIMER) };
    }

    fn tell(&mut self, pending: bool) {
        (self.sink)(Event::Delaying(pending));
    }

    fn take_screenshot(&mut self) {
        self.open(Duration::ZERO, Purpose::Screenshot);
    }

    fn timer_failed(&mut self) {
        self.capture_failed("the timer of the delayed screenshot could not be set".to_owned());
    }

    fn note(&mut self, note: &str) {
        self.emit_notes(vec![note.to_owned()]);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Delays, cancel, start, tick};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        Began(Duration),
        Showed(u64),
        Timer(Duration),
        Ended,
        Told(bool),
        Screenshot,
        Failed,
        Noted,
    }

    #[derive(Default)]
    struct Recorded {
        log: Vec<Step>,
        busy: bool,
        timer_breaks: bool,
        length: Option<Duration>,
        elapsed: Duration,
    }

    impl Delays for Recorded {
        fn may_start(&self) -> bool {
            !self.busy
        }

        fn begin(&mut self, length: Duration) {
            self.log.push(Step::Began(length));
            self.length = Some(length);
            self.elapsed = Duration::ZERO;
        }

        fn left(&self) -> Option<Duration> {
            self.length
                .map(|length| length.saturating_sub(self.elapsed))
        }

        fn show(&mut self, number: u64) {
            self.log.push(Step::Showed(number));
        }

        fn wake_after(&mut self, after: Duration) -> bool {
            self.log.push(Step::Timer(after));
            !self.timer_breaks
        }

        fn end(&mut self) {
            self.log.push(Step::Ended);
            self.length = None;
        }

        fn tell(&mut self, pending: bool) {
            self.log.push(Step::Told(pending));
        }

        fn take_screenshot(&mut self) {
            self.log.push(Step::Screenshot);
        }

        fn timer_failed(&mut self) {
            self.log.push(Step::Failed);
        }

        fn note(&mut self, _: &str) {
            self.log.push(Step::Noted);
        }
    }

    const SECOND: Duration = Duration::from_secs(1);

    fn after(delays: &mut Recorded, elapsed: Duration) -> Vec<Step> {
        delays.log.clear();
        delays.elapsed = elapsed;
        tick(delays);
        std::mem::take(&mut delays.log)
    }

    #[test]
    fn an_overlay_that_cannot_open_refuses_the_delay_before_anything_shows() {
        let mut delays = Recorded {
            busy: true,
            ..Recorded::default()
        };
        start(&mut delays, Duration::from_secs(3));
        assert_eq!(delays.log, [Step::Noted]);
        assert!(delays.left().is_none());
        assert!(after(&mut delays, Duration::from_secs(3)).is_empty());
    }

    #[test]
    fn the_seconds_left_change_once_a_second_and_the_end_takes_one_screenshot() {
        let mut delays = Recorded::default();
        start(&mut delays, Duration::from_secs(3));
        assert_eq!(
            delays.log,
            [
                Step::Began(Duration::from_secs(3)),
                Step::Told(true),
                Step::Showed(3),
                Step::Timer(SECOND),
            ]
        );
        assert_eq!(
            after(&mut delays, SECOND),
            [Step::Showed(2), Step::Timer(SECOND)]
        );
        assert_eq!(
            after(&mut delays, Duration::from_millis(2500)),
            [Step::Showed(1), Step::Timer(Duration::from_millis(500))]
        );
        assert_eq!(
            after(&mut delays, Duration::from_secs(3)),
            [Step::Ended, Step::Told(false), Step::Screenshot]
        );
        assert!(after(&mut delays, Duration::from_secs(4)).is_empty());
    }

    #[test]
    fn an_end_reached_while_the_overlay_cannot_open_takes_nothing_and_says_so() {
        let mut delays = Recorded::default();
        start(&mut delays, Duration::from_secs(3));
        delays.busy = true;
        assert_eq!(
            after(&mut delays, Duration::from_secs(3)),
            [Step::Ended, Step::Told(false), Step::Noted]
        );
        assert!(delays.left().is_none());
    }

    #[test]
    fn a_timer_that_cannot_be_set_ends_the_delay_and_reports_it_instead_of_taking_the_screenshot() {
        let mut delays = Recorded {
            timer_breaks: true,
            ..Recorded::default()
        };
        start(&mut delays, Duration::from_secs(5));
        assert_eq!(
            delays.log,
            [
                Step::Began(Duration::from_secs(5)),
                Step::Told(true),
                Step::Showed(5),
                Step::Timer(SECOND),
                Step::Ended,
                Step::Told(false),
                Step::Failed,
            ]
        );
        assert!(delays.left().is_none());
        assert!(after(&mut delays, Duration::from_secs(5)).is_empty());
    }

    #[test]
    fn a_timer_lost_halfway_ends_the_delay_the_same_way() {
        let mut delays = Recorded::default();
        start(&mut delays, Duration::from_secs(5));
        delays.timer_breaks = true;
        assert_eq!(
            after(&mut delays, SECOND),
            [
                Step::Showed(4),
                Step::Timer(SECOND),
                Step::Ended,
                Step::Told(false),
                Step::Failed,
            ]
        );
        assert!(delays.left().is_none());
    }

    #[test]
    fn a_cancelled_delay_ends_once_and_a_late_timer_takes_nothing() {
        let mut delays = Recorded::default();
        start(&mut delays, Duration::from_secs(10));
        delays.log.clear();
        cancel(&mut delays);
        cancel(&mut delays);
        assert_eq!(delays.log, [Step::Ended, Step::Told(false)]);
        assert!(after(&mut delays, Duration::from_secs(10)).is_empty());
    }

    #[test]
    fn a_delay_asked_again_ends_the_first_before_starting_over() {
        let mut delays = Recorded::default();
        start(&mut delays, Duration::from_secs(10));
        delays.elapsed = Duration::from_secs(4);
        delays.log.clear();
        start(&mut delays, Duration::from_secs(3));
        assert_eq!(
            delays.log,
            [
                Step::Ended,
                Step::Told(false),
                Step::Began(Duration::from_secs(3)),
                Step::Told(true),
                Step::Showed(3),
                Step::Timer(SECOND),
            ]
        );
    }

    #[test]
    fn a_delay_asked_while_the_overlay_cannot_open_still_ends_the_one_running() {
        let mut delays = Recorded::default();
        start(&mut delays, Duration::from_secs(10));
        delays.busy = true;
        delays.log.clear();
        start(&mut delays, Duration::from_secs(3));
        assert_eq!(delays.log, [Step::Ended, Step::Told(false), Step::Noted]);
        assert!(delays.left().is_none());
    }
}
