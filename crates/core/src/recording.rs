use std::time::Duration;

use thiserror::Error;

use crate::geometry::{Point, Rect};

pub const FRAME_RATE: u32 = 15;
pub const LIVE_RATE: u32 = 30;
pub const LONGEST_SIDE: u32 = 1280;
pub const TIME_LIMIT: Duration = Duration::from_secs(60);
const LARGEST_BUDGET: u64 = 1 << 30;
const MOST_PREPARING_THREADS: usize = 3;
const THREADS_KEPT_FREE: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum RecordingStop {
    #[error("it reached its time limit of {} seconds", .0.as_secs())]
    TimeLimit(Duration),
    #[error("the memory set aside for it is full")]
    Memory,
    #[error("the display changed or was locked")]
    Display,
}

impl RecordingStop {
    pub fn locale_id(&self) -> &'static str {
        match self {
            Self::TimeLimit(_) => "recording-stopped-time-limit",
            Self::Memory => "recording-stopped-memory",
            Self::Display => "recording-stopped-display",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pauses {
    total: Duration,
    since: Option<Duration>,
}

impl Pauses {
    pub fn is_paused(&self) -> bool {
        self.since.is_some()
    }

    pub fn pause(&mut self, at: Duration) {
        if self.since.is_none() {
            self.since = Some(at);
        }
    }

    pub fn resume(&mut self, at: Duration) {
        if let Some(since) = self.since.take() {
            self.total += at.saturating_sub(since);
        }
    }

    pub fn recorded(&self, at: Duration) -> Duration {
        let ongoing = self
            .since
            .map_or(Duration::ZERO, |since| at.saturating_sub(since));
        at.saturating_sub(self.total + ongoing)
    }
}

pub fn frame_interval() -> Duration {
    Duration::from_nanos(1_000_000_000 / u64::from(FRAME_RATE))
}

pub fn live_interval() -> Duration {
    Duration::from_nanos(1_000_000_000 / u64::from(LIVE_RATE))
}

pub fn output_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= LONGEST_SIDE {
        return (width, height);
    }
    let scaled = |side: u32| {
        let side = (u64::from(side) * u64::from(LONGEST_SIDE) + u64::from(longest) / 2)
            / u64::from(longest);
        u32::try_from(side).unwrap_or(LONGEST_SIDE).max(1)
    };
    (scaled(width), scaled(height))
}

pub fn budget(available_bytes: Option<u64>) -> usize {
    let quarter = available_bytes.map_or(LARGEST_BUDGET / 4, |available| available / 4);
    usize::try_from(quarter.min(LARGEST_BUDGET)).unwrap_or(usize::MAX)
}

pub fn preparing_threads(available: usize) -> usize {
    available
        .saturating_sub(THREADS_KEPT_FREE)
        .clamp(1, MOST_PREPARING_THREADS)
}

pub fn area_on_one_monitor(selection: Rect, monitors: &[Rect]) -> Option<(usize, Rect)> {
    monitors
        .iter()
        .enumerate()
        .filter_map(|(index, monitor)| Some((index, selection.intersection(monitor)?)))
        .max_by_key(|(_, part)| u64::from(part.width()) * u64::from(part.height()))
}

pub fn border(area: Rect, monitor: Rect, thickness: i32) -> [Rect; 4] {
    let outer = Rect::new(
        (area.left - thickness).max(monitor.left),
        (area.top - thickness).max(monitor.top),
        (area.right + thickness).min(monitor.right),
        (area.bottom + thickness).min(monitor.bottom),
    );
    let inner_left = (outer.left + thickness).min(outer.right);
    let inner_top = (outer.top + thickness).min(outer.bottom);
    let inner_right = (outer.right - thickness).max(inner_left);
    let inner_bottom = (outer.bottom - thickness).max(inner_top);
    [
        Rect::new(outer.left, outer.top, outer.right, inner_top),
        Rect::new(outer.left, inner_bottom, outer.right, outer.bottom),
        Rect::new(outer.left, inner_top, inner_left, inner_bottom),
        Rect::new(inner_right, inner_top, outer.right, inner_bottom),
    ]
}

pub fn controls_at(area: Rect, work: Rect, width: u32, height: u32, gap: i32) -> Point {
    let width = i32::try_from(width).unwrap_or(i32::MAX);
    let height = i32::try_from(height).unwrap_or(i32::MAX);
    let area_width = i32::try_from(area.width()).unwrap_or(i32::MAX);
    let x = (area.left + (area_width - width) / 2)
        .min(work.right - width)
        .max(work.left);
    let below = area.bottom + gap;
    let above = area.top - gap - height;
    let y = if below + height <= work.bottom {
        below
    } else if above >= work.top {
        above
    } else {
        (area.bottom - gap - height)
            .min(work.bottom - height)
            .max(work.top)
    };
    Point::new(x, y)
}

pub fn keep_within(at: Point, width: i32, height: i32, work: Rect) -> Point {
    Point::new(
        at.x.min(work.right - width).max(work.left),
        at.y.min(work.bottom - height).max(work.top),
    )
}

pub fn clock_text(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifteen_frames_fit_in_a_second() {
        assert_eq!(
            frame_interval() * FRAME_RATE,
            Duration::from_nanos(999_999_990)
        );
    }

    #[test]
    fn a_small_area_keeps_its_size_and_a_large_one_shrinks_to_the_longest_side() {
        assert_eq!(output_size(1280, 720), (1280, 720));
        assert_eq!(output_size(300, 200), (300, 200));
        assert_eq!(output_size(2560, 1440), (1280, 720));
        assert_eq!(output_size(1440, 2560), (720, 1280));
        assert_eq!(output_size(3000, 1001), (1280, 427));
        assert_eq!(output_size(100_000, 1), (1280, 1));
    }

    #[test]
    fn the_budget_is_a_quarter_of_the_free_memory_up_to_a_gigabyte() {
        assert_eq!(budget(Some(1 << 30)), 1 << 28);
        assert_eq!(budget(Some(12 << 30)), 1 << 30);
        assert_eq!(budget(Some(0)), 0);
        assert_eq!(budget(None), 1 << 28);
    }

    #[test]
    fn two_threads_are_left_for_capture_and_writing() {
        assert_eq!(preparing_threads(1), 1);
        assert_eq!(preparing_threads(2), 1);
        assert_eq!(preparing_threads(4), 2);
        assert_eq!(preparing_threads(5), 3);
        assert_eq!(preparing_threads(12), 3);
    }

    #[test]
    fn a_selection_over_two_monitors_is_recorded_on_the_one_holding_most_of_it() {
        let monitors = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 4480, 1440)];
        assert_eq!(
            area_on_one_monitor(Rect::new(1800, 100, 2400, 500), &monitors),
            Some((1, Rect::new(1920, 100, 2400, 500)))
        );
        assert_eq!(
            area_on_one_monitor(Rect::new(1000, 100, 2000, 500), &monitors),
            Some((0, Rect::new(1000, 100, 1920, 500)))
        );
        assert_eq!(
            area_on_one_monitor(Rect::new(5000, 0, 5100, 100), &monitors),
            None
        );
    }

    #[test]
    fn the_border_lies_around_the_area() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        assert_eq!(
            border(Rect::new(100, 100, 300, 200), monitor, 2),
            [
                Rect::new(98, 98, 302, 100),
                Rect::new(98, 200, 302, 202),
                Rect::new(98, 100, 100, 200),
                Rect::new(300, 100, 302, 200),
            ]
        );
    }

    #[test]
    fn at_a_monitor_edge_the_border_moves_inside_the_area() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        let [top, bottom, left, right] = border(monitor, monitor, 3);
        assert_eq!(top, Rect::new(0, 0, 1920, 3));
        assert_eq!(bottom, Rect::new(0, 1077, 1920, 1080));
        assert_eq!(left, Rect::new(0, 3, 3, 1077));
        assert_eq!(right, Rect::new(1917, 3, 1920, 1077));
    }

    #[test]
    fn a_tiny_area_still_gets_a_border_without_negative_sides() {
        for side in border(Rect::new(10, 10, 11, 11), Rect::new(0, 0, 100, 100), 4) {
            assert!(
                side.right >= side.left && side.bottom >= side.top,
                "{side:?}"
            );
        }
    }

    #[test]
    fn the_controls_go_below_the_area_then_above_then_inside() {
        let work = Rect::new(0, 0, 1920, 1040);
        assert_eq!(
            controls_at(Rect::new(100, 100, 500, 400), work, 200, 40, 8),
            Point::new(200, 408)
        );
        assert_eq!(
            controls_at(Rect::new(100, 600, 500, 1030), work, 200, 40, 8),
            Point::new(200, 552)
        );
        assert_eq!(
            controls_at(Rect::new(0, 0, 1920, 1040), work, 200, 40, 8),
            Point::new(860, 992)
        );
    }

    #[test]
    fn the_controls_stay_on_the_monitor_beside_an_edge() {
        let work = Rect::new(0, 0, 1920, 1040);
        assert_eq!(
            controls_at(Rect::new(1880, 100, 1920, 200), work, 200, 40, 8),
            Point::new(1720, 208)
        );
        assert_eq!(
            controls_at(Rect::new(0, 100, 40, 200), work, 200, 40, 8),
            Point::new(0, 208)
        );
    }

    #[test]
    fn dragged_controls_stay_inside_the_work_area() {
        let work = Rect::new(0, 0, 1920, 1040);
        assert_eq!(
            keep_within(Point::new(500, 300), 200, 40, work),
            Point::new(500, 300)
        );
        assert_eq!(
            keep_within(Point::new(-50, -10), 200, 40, work),
            Point::new(0, 0)
        );
        assert_eq!(
            keep_within(Point::new(1900, 1030), 200, 40, work),
            Point::new(1720, 1000)
        );
        let second = Rect::new(1920, -200, 4480, 1240);
        assert_eq!(
            keep_within(Point::new(1000, 0), 200, 40, second),
            Point::new(1920, 0)
        );
    }

    #[test]
    fn controls_wider_than_the_work_area_keep_their_start_inside() {
        let work = Rect::new(100, 100, 250, 150);
        assert_eq!(
            keep_within(Point::new(400, 400), 200, 80, work),
            Point::new(100, 100)
        );
    }

    #[test]
    fn the_clock_shows_minutes_and_seconds() {
        assert_eq!(clock_text(Duration::ZERO), "0:00");
        assert_eq!(clock_text(Duration::from_millis(7_900)), "0:07");
        assert_eq!(clock_text(TIME_LIMIT), "1:00");
        assert_eq!(clock_text(Duration::from_secs(605)), "10:05");
    }

    #[test]
    fn time_paused_is_left_out_of_the_recording() {
        let seconds = Duration::from_secs;
        let mut pauses = Pauses::default();
        assert_eq!(pauses.recorded(seconds(5)), seconds(5));
        pauses.pause(seconds(5));
        assert!(pauses.is_paused());
        assert_eq!(pauses.recorded(seconds(9)), seconds(5));
        pauses.resume(seconds(12));
        assert!(!pauses.is_paused());
        assert_eq!(pauses.recorded(seconds(12)), seconds(5));
        assert_eq!(pauses.recorded(seconds(20)), seconds(13));
        pauses.pause(seconds(20));
        pauses.resume(seconds(21));
        assert_eq!(pauses.recorded(seconds(30)), seconds(22));
    }

    #[test]
    fn a_second_pause_or_resume_changes_nothing() {
        let seconds = Duration::from_secs;
        let mut pauses = Pauses::default();
        pauses.resume(seconds(3));
        assert_eq!(pauses, Pauses::default());
        pauses.pause(seconds(4));
        pauses.pause(seconds(6));
        assert_eq!(pauses.recorded(seconds(8)), seconds(4));
        pauses.resume(seconds(8));
        pauses.resume(seconds(9));
        assert_eq!(pauses.recorded(seconds(10)), seconds(6));
    }

    #[test]
    fn a_clock_read_before_the_pause_never_goes_below_zero() {
        let mut pauses = Pauses::default();
        pauses.pause(Duration::from_secs(5));
        assert_eq!(
            pauses.recorded(Duration::from_secs(4)),
            Duration::from_secs(4)
        );
        assert_eq!(pauses.recorded(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn a_time_limit_stop_names_the_limit() {
        let stop = RecordingStop::TimeLimit(TIME_LIMIT);
        assert_eq!(stop.to_string(), "it reached its time limit of 60 seconds");
        assert_eq!(stop.locale_id(), "recording-stopped-time-limit");
    }
}
