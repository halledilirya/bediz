//! A waitable timer, high resolution where Windows offers one, that paces frame captures, keeps
//! them paced while a menu's modal loop holds the thread, and ends that menu when they stop.

use std::any::Any;
use std::cell::Cell;
use std::ffi::c_void;
use std::ops::ControlFlow;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CancelWaitableTimer, CreateWaitableTimerExW,
    SetWaitableTimer, TIMER_ALL_ACCESS, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{EndMenu, KillTimer, SetTimer, USER_TIMER_MINIMUM};
use windows::core::{Error, PCWSTR, Result};

use crate::windows::window_class::guarded;

pub struct Timer(HANDLE);

struct Paced {
    timer: HANDLE,
    tick: *mut c_void,
    run: unsafe fn(*mut c_void) -> ControlFlow<()>,
}

enum Pace {
    Kept(Paced),
    Unwound(Box<dyn Any + Send>),
}

thread_local! {
    static PACE: Cell<Option<Pace>> = const { Cell::new(None) };
}

struct Pacing(usize);

impl Pacing {
    fn end(self) -> Option<Pace> {
        PACE.with(Cell::take)
    }
}

impl Drop for Pacing {
    fn drop(&mut self) {
        let _ = unsafe { KillTimer(None, self.0) };
        PACE.with(|pace| pace.set(None));
    }
}

impl Paced {
    fn new<F: FnMut() -> ControlFlow<()>>(timer: HANDLE, tick: &mut F) -> Self {
        Self {
            timer,
            tick: std::ptr::from_mut(tick).cast(),
            run: run::<F>,
        }
    }
}

// A modal loop keeps the runtime from waiting on the timer but runs a thread timer's procedure.
pub fn keep_pace<T, F: FnMut() -> ControlFlow<()>>(
    timer: HANDLE,
    mut tick: F,
    modal: impl FnOnce() -> T,
) -> Result<T> {
    let started = unsafe { SetTimer(None, 0, USER_TIMER_MINIMUM, Some(on_modal_tick)) };
    hold(started, Paced::new(timer, &mut tick), modal)
}

fn hold<T>(started: usize, paced: Paced, modal: impl FnOnce() -> T) -> Result<T> {
    if started == 0 {
        return Err(Error::from_thread());
    }
    let pacing = Pacing(started);
    PACE.with(|pace| pace.set(Some(Pace::Kept(paced))));
    let held = modal();
    if let Some(Pace::Unwound(unwound)) = pacing.end() {
        resume_unwind(unwound);
    }
    Ok(held)
}

unsafe fn run<F: FnMut() -> ControlFlow<()>>(tick: *mut c_void) -> ControlFlow<()> {
    unsafe { (*tick.cast::<F>())() }
}

unsafe extern "system" fn on_modal_tick(_: HWND, _: u32, _: usize, _: u32) {
    guarded((), || {
        let pace = ticked(PACE.with(Cell::take));
        PACE.with(|slot| slot.set(pace));
    });
}

// A panicking tick ends the menu too and unwinds once it has closed, as it would outside a menu.
fn ticked(pace: Option<Pace>) -> Option<Pace> {
    let paced = match pace {
        Some(Pace::Kept(paced)) => paced,
        ended => return ended,
    };
    if unsafe { WaitForSingleObject(paced.timer, 0) } != WAIT_OBJECT_0 {
        return Some(Pace::Kept(paced));
    }
    let after = match catch_unwind(AssertUnwindSafe(|| unsafe { (paced.run)(paced.tick) })) {
        Ok(ControlFlow::Continue(())) => return Some(Pace::Kept(paced)),
        Ok(ControlFlow::Break(())) => None,
        Err(unwound) => Some(Pace::Unwound(unwound)),
    };
    let _ = unsafe { EndMenu() };
    after
}

impl Timer {
    pub fn start(first: Duration, period: Duration) -> Result<Self> {
        let timer = Self::created()?;
        let due = -i64::try_from(first.max(period).as_nanos() / 100).unwrap_or(i64::MAX);
        let milliseconds = i32::try_from(period.as_millis()).unwrap_or(i32::MAX).max(1);
        unsafe { SetWaitableTimer(timer.0, &due, milliseconds, None, None, false) }?;
        Ok(timer)
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    fn created() -> Result<Self> {
        let create = |flags| unsafe {
            CreateWaitableTimerExW(None, PCWSTR::null(), flags, TIMER_ALL_ACCESS.0)
        };
        Ok(Self(
            create(CREATE_WAITABLE_TIMER_HIGH_RESOLUTION).or_else(|_| create(0))?,
        ))
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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use windows::Win32::Foundation::{ERROR_NOT_ENOUGH_QUOTA, SetLastError, WAIT_TIMEOUT};
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};

    use super::*;

    fn dispatch_for(time: Duration) {
        let until = Instant::now() + time;
        let mut message = MSG::default();
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(2));
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe { DispatchMessageW(&message) };
            }
        }
    }

    fn ticking(period: u64) -> Timer {
        let Ok(timer) = Timer::start(Duration::ZERO, Duration::from_millis(period)) else {
            panic!("a waitable timer is made");
        };
        timer
    }

    #[test]
    fn a_loop_that_holds_the_thread_still_takes_the_ticks() {
        let timer = ticking(20);
        let mut ticks = 0;
        let held = keep_pace(
            timer.handle(),
            || {
                ticks += 1;
                ControlFlow::Continue(())
            },
            || {
                dispatch_for(Duration::from_millis(300));
                "held"
            },
        );
        assert_eq!(held.ok(), Some("held"));
        assert!(ticks >= 3, "{ticks} ticks were taken");
    }

    #[test]
    fn a_tick_that_stops_is_the_last_one_taken() {
        let timer = ticking(10);
        let mut ticks = 0;
        let held = keep_pace(
            timer.handle(),
            || {
                ticks += 1;
                if ticks == 2 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
            || dispatch_for(Duration::from_millis(300)),
        );
        assert!(held.is_ok());
        assert_eq!(ticks, 2);
        assert!(PACE.with(Cell::take).is_none());
    }

    #[test]
    fn a_tick_that_panics_is_the_last_one_and_unwinds_once_the_loop_has_ended() {
        let timer = ticking(10);
        let mut ticks = 0;
        let mut ended = false;
        let unwound = std::panic::catch_unwind(AssertUnwindSafe(|| {
            keep_pace(
                timer.handle(),
                || -> ControlFlow<()> {
                    ticks += 1;
                    panic!("the tick failed")
                },
                || {
                    dispatch_for(Duration::from_millis(150));
                    ended = true;
                },
            )
        }));
        let Err(unwound) = unwound else {
            panic!("the tick's panic goes on unwinding");
        };
        assert_eq!(unwound.downcast_ref::<&str>(), Some(&"the tick failed"));
        assert!(ended);
        assert_eq!(ticks, 1);
        assert!(PACE.with(Cell::take).is_none());
    }

    #[test]
    fn a_thread_timer_that_does_not_start_keeps_the_loop_from_starting() {
        let mut tick = || ControlFlow::<()>::Continue(());
        let mut entered = false;
        unsafe { SetLastError(ERROR_NOT_ENOUGH_QUOTA) };
        let held = hold(0, Paced::new(HANDLE::default(), &mut tick), || {
            entered = true;
        });
        assert_eq!(
            held.map_err(|error| error.code()),
            Err(ERROR_NOT_ENOUGH_QUOTA.to_hresult())
        );
        assert!(!entered);
        assert!(PACE.with(Cell::take).is_none());
    }

    #[test]
    fn nothing_is_left_to_tick_once_the_loop_ends_even_in_a_panic() {
        let timer = ticking(10);
        let handle = timer.handle();
        let going = || ControlFlow::Continue(());
        assert!(keep_pace(handle, going, || ()).is_ok());
        assert!(PACE.with(Cell::take).is_none());
        let unwound: std::thread::Result<Result<()>> =
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                keep_pace(handle, going, || panic!("the loop failed"))
            }));
        assert!(unwound.is_err());
        assert!(PACE.with(Cell::take).is_none());
        dispatch_for(Duration::from_millis(50));
    }

    #[test]
    fn a_tick_taken_once_is_gone() {
        let Ok(timer) = Timer::created() else {
            panic!("a waitable timer is made");
        };
        let once = unsafe { SetWaitableTimer(timer.handle(), &-10_000, 0, None, None, false) };
        assert!(once.is_ok());
        let first = unsafe { WaitForSingleObject(timer.handle(), 5000) };
        let again = unsafe { WaitForSingleObject(timer.handle(), 0) };
        assert_eq!((first, again), (WAIT_OBJECT_0, WAIT_TIMEOUT));
    }
}
