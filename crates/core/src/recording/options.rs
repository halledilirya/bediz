//! A recording's options and the values offered for its frame rate, longest side and time limit.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{0} is not one of the offered values")]
pub struct NotOffered(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct FrameRate(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Option<u32>", into = "Option<u32>")]
pub struct LongestSide(Option<u32>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct TimeLimit(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordingOptions {
    pub frame_rate: FrameRate,
    pub longest_side: LongestSide,
    pub time_limit: TimeLimit,
    pub cursor: bool,
    pub countdown: bool,
    pub copy: bool,
}

impl FrameRate {
    pub const OFFERED: [u32; 5] = [10, 15, 20, 25, 30];

    pub const fn per_second(self) -> u32 {
        self.0
    }

    pub fn interval(self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.0))
    }
}

impl Default for FrameRate {
    fn default() -> Self {
        Self(15)
    }
}

impl TryFrom<u32> for FrameRate {
    type Error = NotOffered;

    fn try_from(rate: u32) -> Result<Self, NotOffered> {
        offered(&Self::OFFERED, rate).map(Self)
    }
}

impl From<FrameRate> for u32 {
    fn from(rate: FrameRate) -> Self {
        rate.0
    }
}

impl LongestSide {
    pub const OFFERED: [u32; 4] = [640, 960, 1280, 1920];
    pub const KEPT: Self = Self(None);

    pub const fn limit(self) -> Option<u32> {
        self.0
    }
}

impl Default for LongestSide {
    fn default() -> Self {
        Self(Some(1280))
    }
}

impl TryFrom<Option<u32>> for LongestSide {
    type Error = NotOffered;

    fn try_from(side: Option<u32>) -> Result<Self, NotOffered> {
        side.map(|side| offered(&Self::OFFERED, side))
            .transpose()
            .map(Self)
    }
}

impl From<LongestSide> for Option<u32> {
    fn from(side: LongestSide) -> Self {
        side.0
    }
}

impl TimeLimit {
    pub const OFFERED: [u32; 5] = [15, 30, 60, 120, 300];

    pub fn duration(self) -> Duration {
        Duration::from_secs(u64::from(self.0))
    }
}

impl Default for TimeLimit {
    fn default() -> Self {
        Self(60)
    }
}

impl TryFrom<u32> for TimeLimit {
    type Error = NotOffered;

    fn try_from(seconds: u32) -> Result<Self, NotOffered> {
        offered(&Self::OFFERED, seconds).map(Self)
    }
}

impl From<TimeLimit> for u32 {
    fn from(limit: TimeLimit) -> Self {
        limit.0
    }
}

impl Default for RecordingOptions {
    fn default() -> Self {
        Self {
            frame_rate: FrameRate::default(),
            longest_side: LongestSide::default(),
            time_limit: TimeLimit::default(),
            cursor: true,
            countdown: false,
            copy: true,
        }
    }
}

fn offered(values: &[u32], value: u32) -> Result<u32, NotOffered> {
    if values.contains(&value) {
        Ok(value)
    } else {
        Err(NotOffered(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_offered_values_are_taken() {
        for rate in FrameRate::OFFERED {
            assert_eq!(FrameRate::try_from(rate).unwrap().per_second(), rate);
        }
        for rate in [0, 1, 14, 60] {
            assert!(FrameRate::try_from(rate).is_err(), "{rate}");
        }
        for side in LongestSide::OFFERED {
            assert_eq!(
                LongestSide::try_from(Some(side)).unwrap().limit(),
                Some(side)
            );
        }
        assert_eq!(LongestSide::try_from(None).unwrap(), LongestSide::KEPT);
        assert!(LongestSide::try_from(Some(0)).is_err());
        assert!(LongestSide::try_from(Some(1000)).is_err());
        for seconds in TimeLimit::OFFERED {
            assert_eq!(
                TimeLimit::try_from(seconds).unwrap().duration(),
                Duration::from_secs(u64::from(seconds))
            );
        }
        assert!(TimeLimit::try_from(0).is_err());
        assert!(TimeLimit::try_from(3600).is_err());
    }

    #[test]
    fn the_defaults_are_the_proposed_ones() {
        let options = RecordingOptions::default();
        assert_eq!(options.frame_rate.per_second(), 15);
        assert_eq!(options.longest_side.limit(), Some(1280));
        assert_eq!(options.time_limit.duration(), Duration::from_secs(60));
        assert!(options.cursor);
        assert!(!options.countdown);
        assert!(options.copy);
        assert!(FrameRate::OFFERED.contains(&FrameRate::default().per_second()));
        assert!(TimeLimit::OFFERED.contains(&TimeLimit::default().into()));
    }

    #[test]
    fn every_offered_rate_fits_its_frames_in_a_second() {
        for rate in FrameRate::OFFERED {
            let rate = FrameRate::try_from(rate).unwrap();
            let second = rate.interval() * rate.per_second();
            assert!(second <= Duration::from_secs(1), "{rate:?}");
            assert!(
                Duration::from_secs(1) - second < Duration::from_micros(1),
                "{rate:?}"
            );
        }
    }

    #[test]
    fn the_values_are_written_as_plain_numbers() {
        let rate: FrameRate = serde_json::from_str("20").unwrap();
        assert_eq!(serde_json::to_string(&rate).unwrap(), "20");
        let kept: LongestSide = serde_json::from_str("null").unwrap();
        assert_eq!(kept, LongestSide::KEPT);
        assert_eq!(serde_json::to_string(&kept).unwrap(), "null");
        let limit: TimeLimit = serde_json::from_str("120").unwrap();
        assert_eq!(serde_json::to_string(&limit).unwrap(), "120");
        assert!(serde_json::from_str::<FrameRate>("16").is_err());
        assert!(serde_json::from_str::<LongestSide>("1281").is_err());
        assert!(serde_json::from_str::<TimeLimit>("\"60\"").is_err());
    }
}
