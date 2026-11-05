use std::time::Duration;

use bediz_core::measurement::duration_from_ticks;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::SystemInformation::GetTickCount;

#[derive(Clone, Copy, Debug)]
pub struct Tick(i64);

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    frequency: i64,
}

impl Clock {
    pub fn new() -> Self {
        let mut frequency = 0;
        if unsafe { QueryPerformanceFrequency(&mut frequency) }.is_err() {
            frequency = 0;
        }
        Self { frequency }
    }

    pub fn now(&self) -> Tick {
        let mut ticks = 0;
        if unsafe { QueryPerformanceCounter(&mut ticks) }.is_err() {
            ticks = 0;
        }
        Tick(ticks)
    }

    pub fn since(&self, start: Tick) -> Duration {
        duration_from_ticks(self.now().0.saturating_sub(start.0), self.frequency)
    }
}

pub fn message_age(message_time: u32) -> Duration {
    let now = unsafe { GetTickCount() };
    Duration::from_millis(u64::from(now.wrapping_sub(message_time)))
}
