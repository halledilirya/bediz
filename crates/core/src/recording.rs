mod options;

use std::time::Duration;

use serde::Serialize;
use thiserror::Error;

use crate::geometry::{Point, Rect};

pub use options::{FrameRate, LongestSide, NotOffered, RecordingOptions, TimeLimit};

pub const LIVE_RATE: u32 = 30;
pub const COUNTDOWN: Duration = Duration::from_secs(3);
const MEGABYTE: u64 = 1 << 20;
const GITHUB_LIMIT: u64 = 10 * MEGABYTE;
const X_LIMIT: u64 = 15 * MEGABYTE;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SizeWarning {
    GitHub,
    X,
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

pub fn live_interval() -> Duration {
    Duration::from_nanos(1_000_000_000 / u64::from(LIVE_RATE))
}

pub fn output_size(width: u32, height: u32, longest_side: LongestSide) -> (u32, u32) {
    let longest = width.max(height);
    let Some(limit) = longest_side.limit().filter(|&limit| longest > limit) else {
        return (width, height);
    };
    let scaled = |side: u32| {
        let side =
            (u64::from(side) * u64::from(limit) + u64::from(longest) / 2) / u64::from(longest);
        u32::try_from(side).unwrap_or(limit).max(1)
    };
    (scaled(width), scaled(height))
}

pub fn countdown_number(remaining: Duration) -> u64 {
    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
}

pub fn size_warning(bytes: u64) -> Option<SizeWarning> {
    if bytes > X_LIMIT {
        Some(SizeWarning::X)
    } else if bytes > GITHUB_LIMIT {
        Some(SizeWarning::GitHub)
    } else {
        None
    }
}

pub fn written_percent(written: u64, total: u64) -> u8 {
    let percent = u128::from(written) * 100 / u128::from(total.max(1));
    u8::try_from(percent.min(99)).unwrap_or(99)
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

pub fn dashes(length: i32, dash: i32) -> impl Iterator<Item = (i32, i32)> {
    let dash = dash.max(1);
    let period = usize::try_from(dash.saturating_mul(2)).unwrap_or(2);
    (0..length.max(0))
        .step_by(period)
        .map(move |start| (start, start.saturating_add(dash).min(length)))
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

pub fn controls_replacing(previous: Rect, width: u32, height: u32, work: Rect) -> Point {
    let width = i32::try_from(width).unwrap_or(i32::MAX);
    let height = i32::try_from(height).unwrap_or(i32::MAX);
    let centre = previous
        .left
        .saturating_add(i32::try_from(previous.width() / 2).unwrap_or(0));
    keep_within(
        Point::new(centre.saturating_sub(width / 2), previous.top),
        width,
        height,
        work,
    )
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

    fn side(limit: u32) -> LongestSide {
        LongestSide::try_from(Some(limit)).unwrap()
    }

    #[test]
    fn a_small_area_keeps_its_size_and_a_large_one_shrinks_to_the_longest_side() {
        let limit = LongestSide::default();
        assert_eq!(output_size(1280, 720, limit), (1280, 720));
        assert_eq!(output_size(300, 200, limit), (300, 200));
        assert_eq!(output_size(2560, 1440, limit), (1280, 720));
        assert_eq!(output_size(1440, 2560, limit), (720, 1280));
        assert_eq!(output_size(3000, 1001, limit), (1280, 427));
        assert_eq!(output_size(100_000, 1, limit), (1280, 1));
    }

    #[test]
    fn the_longest_side_follows_the_setting_or_keeps_the_area() {
        assert_eq!(output_size(1920, 1080, side(640)), (640, 360));
        assert_eq!(output_size(1920, 1080, side(960)), (960, 540));
        assert_eq!(output_size(1920, 1080, side(1920)), (1920, 1080));
        assert_eq!(output_size(3840, 2160, side(1920)), (1920, 1080));
        assert_eq!(output_size(3840, 2160, LongestSide::KEPT), (3840, 2160));
        assert_eq!(output_size(500, 300, side(640)), (500, 300));
    }

    #[test]
    fn the_countdown_shows_whole_seconds_left_rounded_up() {
        assert_eq!(countdown_number(COUNTDOWN), 3);
        assert_eq!(countdown_number(Duration::from_millis(2_001)), 3);
        assert_eq!(countdown_number(Duration::from_millis(2_000)), 2);
        assert_eq!(countdown_number(Duration::from_millis(1)), 1);
        assert_eq!(countdown_number(Duration::ZERO), 0);
    }

    #[test]
    fn a_gif_above_ten_megabytes_as_explorer_counts_them_is_too_large_for_github() {
        assert_eq!(size_warning(0), None);
        assert_eq!(size_warning(10_485_760), None);
        assert_eq!(size_warning(10_485_761), Some(SizeWarning::GitHub));
        assert_eq!(size_warning(15_728_640), Some(SizeWarning::GitHub));
        assert_eq!(size_warning(15_728_641), Some(SizeWarning::X));
        assert_eq!(
            serde_json::to_string(&SizeWarning::GitHub).unwrap(),
            r#""gitHub""#
        );
    }

    #[test]
    fn the_saved_share_rounds_down_and_stays_below_a_hundred_until_the_file_is_done() {
        assert_eq!(written_percent(0, 4500), 0);
        assert_eq!(written_percent(2249, 4500), 49);
        assert_eq!(written_percent(2250, 4500), 50);
        assert_eq!(written_percent(4499, 4500), 99);
        assert_eq!(written_percent(4500, 4500), 99);
        assert_eq!(written_percent(0, 0), 0);
        assert_eq!(written_percent(u64::MAX, u64::MAX), 99);
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
    fn dashes_and_gaps_alternate_and_the_last_dash_stops_at_the_end() {
        let all = |length, dash| dashes(length, dash).collect::<Vec<_>>();
        assert_eq!(all(20, 4), [(0, 4), (8, 12), (16, 20)]);
        assert_eq!(all(18, 4), [(0, 4), (8, 12), (16, 18)]);
        assert_eq!(all(3, 4), [(0, 3)]);
        assert!(all(0, 4).is_empty());
        assert!(all(-5, 4).is_empty());
        assert_eq!(all(5, 0), [(0, 1), (2, 3), (4, 5)]);
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
    fn the_recording_controls_take_the_place_of_the_moved_ones() {
        let work = Rect::new(0, 0, 1920, 1040);
        assert_eq!(
            controls_replacing(Rect::new(700, 300, 900, 340), 240, 40, work),
            Point::new(680, 300)
        );
        assert_eq!(
            controls_replacing(Rect::new(1720, 1000, 1920, 1040), 240, 40, work),
            Point::new(1680, 1000)
        );
        assert_eq!(
            controls_replacing(Rect::new(0, 0, 200, 40), 240, 40, work),
            Point::new(0, 0)
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
        assert_eq!(clock_text(TimeLimit::default().duration()), "1:00");
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
        let stop = RecordingStop::TimeLimit(TimeLimit::default().duration());
        assert_eq!(stop.to_string(), "it reached its time limit of 60 seconds");
        assert_eq!(stop.locale_id(), "recording-stopped-time-limit");
    }
}
