//! A waitable timer, high resolution where Windows offers one, that paces frame captures.

use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CancelWaitableTimer, CreateWaitableTimerExW,
    SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows::core::{PCWSTR, Result};

pub struct Timer(HANDLE);

impl Timer {
    pub fn start(first: Duration, period: Duration) -> Result<Self> {
        let create = |flags| unsafe {
            CreateWaitableTimerExW(None, PCWSTR::null(), flags, TIMER_ALL_ACCESS.0)
        };
        let timer = Self(create(CREATE_WAITABLE_TIMER_HIGH_RESOLUTION).or_else(|_| create(0))?);
        let due = -i64::try_from(first.max(period).as_nanos() / 100).unwrap_or(i64::MAX);
        let milliseconds = i32::try_from(period.as_millis()).unwrap_or(i32::MAX).max(1);
        unsafe { SetWaitableTimer(timer.0, &due, milliseconds, None, None, false) }?;
        Ok(timer)
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        unsafe {
            let _ = CancelWaitableTimer(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}
