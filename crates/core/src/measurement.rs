use std::fmt::Write;
use std::time::Duration;

use crate::capture::{Allocation, CaptureMethod, Pixels};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    pub working_set_bytes: u64,
    pub commit_bytes: u64,
    pub gpu_local_bytes: u64,
    pub gpu_shared_bytes: u64,
}

impl Memory {
    pub fn log_fields(&self) -> String {
        format!(
            "working_set_mib={} commit_mib={} gpu_local_mib={} gpu_shared_mib={}",
            mebibytes(self.working_set_bytes),
            mebibytes(self.commit_bytes),
            mebibytes(self.gpu_local_bytes),
            mebibytes(self.gpu_shared_bytes)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorCapture {
    pub width: u32,
    pub height: u32,
    pub method: CaptureMethod,
    pub pixels: Pixels,
    pub fresh: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Measurement {
    pub allocation: Allocation,
    pub monitors: Vec<MonitorCapture>,
    pub queued: Duration,
    pub captured: Duration,
    pub listed: Duration,
    pub presented: Duration,
    pub uncloaked: Duration,
    pub focused: Duration,
    pub windows: usize,
    pub memory: Memory,
}

impl Measurement {
    pub fn log_line(&self) -> String {
        let mut monitors = String::new();
        for (index, monitor) in self.monitors.iter().enumerate() {
            if index > 0 {
                monitors.push(',');
            }
            let _ = write!(
                monitors,
                "{}x{}:{}:{}:{}",
                monitor.width,
                monitor.height,
                monitor.method.name(),
                monitor.pixels,
                if monitor.fresh { "fresh" } else { "held" }
            );
        }
        format!(
            "hotkey allocation={} monitors={} queued_ms={} captured_ms={} listed_ms={} presented_ms={} uncloaked_ms={} focused_ms={} windows={} {}",
            self.allocation.name(),
            monitors,
            milliseconds(self.queued),
            milliseconds(self.captured),
            milliseconds(self.listed),
            milliseconds(self.presented),
            milliseconds(self.uncloaked),
            milliseconds(self.focused),
            self.windows,
            self.memory.log_fields()
        )
    }
}

pub fn duration_from_ticks(ticks: i64, frequency: i64) -> Duration {
    let (Ok(ticks), Ok(frequency)) = (u128::try_from(ticks), u128::try_from(frequency)) else {
        return Duration::ZERO;
    };
    let nanos = ticks
        .saturating_mul(1_000_000_000)
        .checked_div(frequency)
        .unwrap_or(0);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

fn milliseconds(duration: Duration) -> String {
    format!("{:.2}", duration.as_secs_f64() * 1000.0)
}

fn mebibytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_048_576.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_convert_with_the_counter_frequency() {
        assert_eq!(
            duration_from_ticks(10_000_000, 10_000_000),
            Duration::from_secs(1)
        );
        assert_eq!(
            duration_from_ticks(12_345, 10_000_000),
            Duration::from_nanos(1_234_500)
        );
    }

    #[test]
    fn invalid_ticks_or_frequency_give_zero() {
        assert_eq!(duration_from_ticks(-5, 10_000_000), Duration::ZERO);
        assert_eq!(duration_from_ticks(5, 0), Duration::ZERO);
        assert_eq!(duration_from_ticks(5, -1), Duration::ZERO);
    }

    #[test]
    fn huge_tick_counts_saturate() {
        assert_eq!(
            duration_from_ticks(i64::MAX, 1),
            Duration::from_nanos(u64::MAX)
        );
    }

    #[test]
    fn log_line_lists_every_field() {
        let measurement = Measurement {
            allocation: Allocation::OnHotkey,
            monitors: vec![
                MonitorCapture {
                    width: 3840,
                    height: 2160,
                    method: CaptureMethod::Duplication,
                    pixels: Pixels::Hdr {
                        sdr_white_level: std::num::NonZeroU32::new(3000).unwrap(),
                    },
                    fresh: false,
                },
                MonitorCapture {
                    width: 1920,
                    height: 1080,
                    method: CaptureMethod::Gdi,
                    pixels: Pixels::Bgra8,
                    fresh: true,
                },
            ],
            queued: Duration::from_millis(1),
            captured: Duration::from_micros(4_210),
            listed: Duration::from_micros(4_800),
            presented: Duration::from_micros(6_020),
            uncloaked: Duration::from_micros(6_100),
            focused: Duration::from_micros(6_350),
            windows: 37,
            memory: Memory {
                working_set_bytes: 12 * 1_048_576,
                commit_bytes: 25 * 1_048_576,
                gpu_local_bytes: 3 * 1_048_576 / 2,
                gpu_shared_bytes: 0,
            },
        };
        assert_eq!(
            measurement.log_line(),
            "hotkey allocation=on-hotkey monitors=3840x2160:duplication:hdr-240nits:held,1920x1080:gdi:bgra8:fresh queued_ms=1.00 captured_ms=4.21 listed_ms=4.80 presented_ms=6.02 uncloaked_ms=6.10 focused_ms=6.35 windows=37 working_set_mib=12.0 commit_mib=25.0 gpu_local_mib=1.5 gpu_shared_mib=0.0"
        );
    }
}
