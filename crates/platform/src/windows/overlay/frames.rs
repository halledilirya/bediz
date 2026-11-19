//! Paces an overlay's frames as Microsoft documents for a waitable swap chain: the next input is
//! read, and the next frame drawn, once the frame before it is on the screen.

use std::cell::Cell;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain1, IDXGISwapChain2};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::core::Interface;

pub const SHOWN_WITHIN_MS: u32 = 100;
// An object that stays signalled must not hold the thread.
const MOST_SIGNALS: u32 = 16;

pub struct Frames {
    shown: Option<Shown>,
    pace: Cell<Pace>,
    late: Cell<bool>,
}

impl Frames {
    pub fn of(swap_chain: &IDXGISwapChain1) -> Self {
        let handle = swap_chain
            .cast::<IDXGISwapChain2>()
            .ok()
            .map(|chain| unsafe { chain.GetFrameLatencyWaitableObject() })
            .filter(|handle| !handle.is_invalid());
        Self {
            shown: handle.map(Shown),
            pace: Cell::default(),
            late: Cell::new(false),
        }
    }

    pub fn start(&self) {
        if let Some(shown) = &self.shown {
            self.pace.set(self.pace.get().started(|| shown.wait(0)));
        }
    }

    pub fn stop(&self) {
        self.pace.set(self.pace.get().stopped());
    }

    pub fn presented(&self) {
        self.pace.set(self.pace.get().presented());
    }

    pub fn wait(&self) {
        let Some(shown) = &self.shown else {
            return;
        };
        let (pace, in_time) = self
            .pace
            .get()
            .waited(|| shown.wait(SHOWN_WITHIN_MS), || shown.wait(0));
        self.pace.set(pace);
        if !in_time {
            self.late.set(true);
        }
    }

    pub fn take_late(&self) -> bool {
        self.late.replace(false)
    }
}

struct Shown(HANDLE);

impl Shown {
    fn wait(&self, millis: u32) -> bool {
        let waited = unsafe { WaitForSingleObject(self.0, millis) };
        waited == WAIT_OBJECT_0
    }
}

impl Drop for Shown {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Pace {
    #[default]
    Hidden,
    Waiting {
        presented: bool,
    },
    Draining,
}

impl Pace {
    // A new swap chain starts signalled, and a frame shown after the overlay hid leaves a signal.
    fn started(self, take: impl FnMut() -> bool) -> Self {
        taken(take);
        Self::Waiting { presented: false }
    }

    fn stopped(self) -> Self {
        Self::Hidden
    }

    fn presented(self) -> Self {
        match self {
            Self::Waiting { .. } => Self::Waiting { presented: true },
            other => other,
        }
    }

    // After a frame not shown in time the signals are still taken, or they would end waits early.
    fn waited(self, wait: impl FnOnce() -> bool, take: impl FnMut() -> bool) -> (Self, bool) {
        match self {
            Self::Waiting { presented: true } => {
                if wait() {
                    (Self::Waiting { presented: false }, true)
                } else {
                    (Self::Draining, false)
                }
            }
            Self::Draining => {
                taken(take);
                (self, true)
            }
            Self::Hidden | Self::Waiting { presented: false } => (self, true),
        }
    }
}

fn taken(mut take: impl FnMut() -> bool) {
    for _ in 0..MOST_SIGNALS {
        if !take() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const READY: Pace = Pace::Waiting { presented: false };

    #[derive(Clone)]
    struct Chain {
        signals: Cell<u32>,
        queued: Cell<u32>,
    }

    impl Chain {
        fn signalled(signals: u32) -> Self {
            Self {
                signals: Cell::new(signals),
                queued: Cell::new(0),
            }
        }

        fn state(&self) -> (u32, u32) {
            (self.queued.get(), self.signals.get())
        }

        fn show(&self) {
            if self.queued.get() > 0 {
                self.queued.set(self.queued.get() - 1);
                self.signals.set(self.signals.get() + 1);
            }
        }

        fn lose(&self) {
            self.queued.set(0);
        }

        fn take(&self) -> bool {
            if self.signals.get() == 0 {
                return false;
            }
            self.signals.set(self.signals.get() - 1);
            true
        }

        fn wait(&self) -> bool {
            if self.signals.get() == 0 {
                self.show();
            }
            self.take()
        }

        fn started(&self, pace: Pace) -> Pace {
            pace.started(|| self.take())
        }

        fn waited(&self, pace: Pace) -> (Pace, bool) {
            pace.waited(|| self.wait(), || self.take())
        }

        fn drawn(&self, pace: Pace) -> (Pace, bool) {
            let (pace, in_time) = self.waited(pace);
            self.show();
            self.queued.set(1);
            (pace.presented(), in_time)
        }
    }

    #[test]
    fn every_wait_ends_once_the_frame_before_it_is_shown() {
        let chain = Chain::signalled(1);
        let started = chain.started(Pace::Hidden);
        assert_eq!(chain.state(), (0, 0));
        let (cloaked, _) = chain.drawn(started);
        let (first_view, in_time) = chain.drawn(cloaked);
        assert!(in_time);
        assert_eq!(chain.state(), (1, 0));
        let mut pace = chain.waited(first_view).0;
        assert_eq!((pace, chain.state()), (READY, (0, 0)));
        for _ in 0..10 {
            pace = chain.drawn(pace).0;
            pace = chain.waited(pace).0;
            assert_eq!((pace, chain.state()), (READY, (0, 0)));
        }
    }

    #[test]
    fn the_signals_there_when_a_showing_starts_are_taken() {
        for signals in [0, 1, 3, MOST_SIGNALS] {
            let chain = Chain::signalled(signals);
            assert_eq!(chain.started(Pace::Hidden), READY);
            assert_eq!(chain.state(), (0, 0));
        }
    }

    #[test]
    fn a_frame_not_shown_in_time_stalls_once_and_its_signal_is_taken_later() {
        let chain = Chain::signalled(1);
        let drawn = chain.drawn(chain.started(Pace::Hidden)).0;
        let (mut pace, in_time) = drawn.waited(|| false, || chain.take());
        assert_eq!((pace, in_time), (Pace::Draining, false));
        for _ in 0..10 {
            let waits = Cell::new(0);
            pace = pace
                .waited(
                    || {
                        waits.set(waits.get() + 1);
                        false
                    },
                    || chain.take(),
                )
                .0;
            pace = chain.drawn(pace).0;
            chain.show();
            assert_eq!((pace, waits.get()), (Pace::Draining, 0));
        }
        pace = chain.waited(pace).0;
        assert_eq!(chain.state(), (0, 0));
        pace = chain.started(pace.stopped());
        assert_eq!(chain.waited(chain.drawn(pace).0), (READY, true));
    }

    #[test]
    fn nothing_is_waited_for_while_the_overlay_is_hidden() {
        let chain = Chain::signalled(1);
        let hidden = chain.drawn(chain.started(Pace::Hidden)).0.stopped();
        let waits = Cell::new(0);
        let (pace, in_time) = hidden.waited(
            || {
                waits.set(waits.get() + 1);
                true
            },
            || chain.take(),
        );
        assert_eq!((pace, in_time, waits.get()), (Pace::Hidden, true, 0));
        assert_eq!(chain.state(), (1, 0));
    }

    #[test]
    fn an_object_that_stays_signalled_ends_the_taking() {
        let takes = Cell::new(0);
        let started = Pace::Hidden.started(|| {
            takes.set(takes.get() + 1);
            true
        });
        assert_eq!((started, takes.get()), (READY, MOST_SIGNALS));
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        Draw,
        Wait,
        Show,
        Lose,
        Slow,
        Start,
        Stop,
    }

    const EVENTS: [Event; 7] = [
        Event::Draw,
        Event::Wait,
        Event::Show,
        Event::Lose,
        Event::Slow,
        Event::Start,
        Event::Stop,
    ];

    const DEPTH: usize = 8;

    #[derive(Clone)]
    struct World {
        chain: Chain,
        pace: Pace,
        clean: bool,
        late: u32,
    }

    impl World {
        fn after(&self, event: Event) -> Self {
            let mut world = self.clone();
            let chain = &world.chain;
            let mut in_time = true;
            match event {
                Event::Draw if world.pace != Pace::Hidden => {
                    (world.pace, in_time) = chain.drawn(world.pace);
                }
                Event::Draw => {}
                Event::Wait => (world.pace, in_time) = chain.waited(world.pace),
                Event::Show => chain.show(),
                Event::Lose => {
                    world.clean &= chain.queued.get() == 0;
                    chain.lose();
                }
                Event::Slow if chain.signals.get() == 0 => {
                    (world.pace, in_time) = world.pace.waited(|| false, || chain.take());
                    world.clean &= in_time;
                }
                Event::Slow => {}
                Event::Start => {
                    world.clean = chain.queued.get() == 0;
                    world.late = 0;
                    world.pace = chain.started(world.pace);
                }
                Event::Stop => world.pace = world.pace.stopped(),
            }
            if !in_time {
                world.late += 1;
            }
            world
        }

        fn check(&self, event: Event, path: &[Event]) {
            let (queued, signals) = self.chain.state();
            assert!(self.late <= 1, "{path:?}");
            assert!(signals <= 3, "{path:?}");
            if let (true, Pace::Waiting { presented }) = (self.clean, self.pace) {
                assert_eq!(signals + queued, u32::from(presented), "{path:?}");
                if event == Event::Wait {
                    assert_eq!((queued, signals), (0, 0), "{path:?}");
                }
            }
        }
    }

    fn explore(world: &World, path: &mut Vec<Event>) {
        if path.len() == DEPTH {
            return;
        }
        for event in EVENTS {
            path.push(event);
            let next = world.after(event);
            next.check(event, path);
            explore(&next, path);
            path.pop();
        }
    }

    #[test]
    fn every_order_of_up_to_eight_events_keeps_the_waits_honest() {
        let world = World {
            chain: Chain::signalled(1),
            pace: Pace::Hidden,
            clean: false,
            late: 0,
        };
        explore(&world, &mut Vec::new());
    }
}
