//! Turns frame times into GIF delays in hundredths of a second, keeping every delay long enough
//! for browsers to honour and splitting any too long for one frame.

use std::time::Duration;

// Browsers play a delay of 0 or 1 hundredths of a second as 10.
pub(super) const MIN_DELAY: u32 = 2;

#[derive(Debug, Default)]
pub(super) struct Timing {
    last_start: Option<u32>,
}

impl Timing {
    pub(super) fn start(&mut self, at: Duration) -> u32 {
        let start = self.earliest(at);
        self.last_start = Some(start);
        start
    }

    pub(super) fn end(&self, at: Duration) -> u32 {
        self.earliest(at)
    }

    fn earliest(&self, at: Duration) -> u32 {
        let wanted = centiseconds(at);
        match self.last_start {
            Some(last) => wanted.max(last.saturating_add(MIN_DELAY)),
            None => wanted,
        }
    }
}

pub(super) fn delays(start: u32, end: u32) -> Vec<u16> {
    let longest = u32::from(u16::MAX);
    let mut rest = end.saturating_sub(start);
    let mut parts = Vec::new();
    while rest > longest {
        let part = if rest - longest >= MIN_DELAY {
            longest
        } else {
            rest - MIN_DELAY
        };
        parts.push(part);
        rest -= part;
    }
    parts.push(rest);
    parts
        .into_iter()
        .map(|part| u16::try_from(part).unwrap_or(u16::MAX))
        .collect()
}

fn centiseconds(at: Duration) -> u32 {
    u32::try_from(at.as_millis().saturating_add(5) / 10).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gaps(starts: &[u32]) -> Vec<u16> {
        starts
            .windows(2)
            .flat_map(|pair| delays(pair[0], pair[1]))
            .collect()
    }

    #[test]
    fn fifteen_frames_a_second_keep_time() {
        let mut timing = Timing::default();
        let starts: Vec<u32> = (0..=15)
            .map(|frame| timing.start(Duration::from_micros(frame * 1_000_000 / 15)))
            .collect();
        assert!(gaps(&starts).iter().all(|delay| *delay == 6 || *delay == 7));
        assert_eq!(starts.first(), Some(&0));
        assert_eq!(starts.last(), Some(&100));
    }

    #[test]
    fn frames_closer_than_the_minimum_are_pushed_apart() {
        let mut timing = Timing::default();
        let starts: Vec<u32> = [0, 5, 10, 15, 100]
            .into_iter()
            .map(|millis| timing.start(Duration::from_millis(millis)))
            .collect();
        assert_eq!(starts, [0, 2, 4, 6, 10]);
        assert_eq!(gaps(&starts), [2, 2, 2, 4]);
    }

    #[test]
    fn pushed_frames_catch_up_with_a_later_one() {
        let mut timing = Timing::default();
        let starts: Vec<u32> = [0, 15, 30, 1_000]
            .into_iter()
            .map(|millis| timing.start(Duration::from_millis(millis)))
            .collect();
        assert_eq!(starts, [0, 2, 4, 100]);
    }

    #[test]
    fn the_end_is_at_least_the_minimum_after_the_last_start() {
        let mut timing = Timing::default();
        timing.start(Duration::from_millis(500));
        assert_eq!(timing.end(Duration::from_millis(500)), 52);
        assert_eq!(timing.end(Duration::from_millis(900)), 90);
    }

    #[test]
    fn a_delay_the_format_holds_is_one_part() {
        assert_eq!(delays(3, 10), [7]);
        assert_eq!(delays(0, 65_535), [65_535]);
    }

    #[test]
    fn longer_delays_are_split_without_losing_time() {
        assert_eq!(delays(0, 65_536), [65_534, 2]);
        assert_eq!(delays(0, 65_537), [65_535, 2]);
        assert_eq!(delays(0, 70_000), [65_535, 4_465]);
        assert_eq!(delays(100, 131_172), [65_535, 65_535, 2]);
    }

    #[test]
    fn half_a_hundredth_rounds_up() {
        assert_eq!(centiseconds(Duration::from_millis(4)), 0);
        assert_eq!(centiseconds(Duration::from_millis(5)), 1);
        assert_eq!(centiseconds(Duration::from_secs(u64::MAX)), u32::MAX);
    }
}
