//! Measures how a GIF was written after its recording stopped, for the line it leaves in the log.

use std::fmt;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) struct Span {
    pub(super) start: Instant,
    pub(super) end: Instant,
}

impl Span {
    fn took(self) -> Duration {
        self.end.saturating_duration_since(self.start)
    }

    fn since(self, stop: Instant) -> Duration {
        self.end.saturating_duration_since(self.start.max(stop))
    }
}

#[derive(Default)]
pub(super) struct Pace {
    pub(super) threads: usize,
    pub(super) canvas: (u32, u32),
    pub(super) frames: u64,
    pub(super) after_stop: u64,
    pub(super) since_stop: Duration,
    pub(super) preparing: Duration,
    pub(super) preparing_since: Duration,
    pub(super) writing: Duration,
    pub(super) writing_since: Duration,
    pub(super) changed: u64,
    pub(super) closing: Duration,
}

impl Pace {
    pub(super) fn new(threads: usize, canvas: (u32, u32)) -> Self {
        Self {
            threads,
            canvas,
            ..Self::default()
        }
    }

    pub(super) fn wrote(
        &mut self,
        preparing: Span,
        writing: Span,
        changed: u64,
        stop: Option<Instant>,
    ) {
        self.frames = self.frames.saturating_add(1);
        let Some(stop) = stop.filter(|stop| writing.end > *stop) else {
            return;
        };
        self.after_stop = self.after_stop.saturating_add(1);
        self.preparing = self.preparing.saturating_add(preparing.took());
        self.preparing_since = self.preparing_since.saturating_add(preparing.since(stop));
        self.writing = self.writing.saturating_add(writing.took());
        self.writing_since = self.writing_since.saturating_add(writing.since(stop));
        self.changed = self.changed.saturating_add(changed);
    }

    pub(super) fn ended(&mut self, stop: Option<Instant>, at: Instant) {
        if let Some(stop) = stop {
            self.since_stop = at.saturating_duration_since(stop);
        }
    }

    pub(super) fn closed_in(&mut self, closing: Duration) {
        self.closing = closing;
    }
}

impl fmt::Display for Pace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (width, height) = self.canvas;
        write!(
            f,
            "the GIF was written: {} frames of {width}x{height}",
            self.frames
        )?;
        if self.after_stop == 0 {
            f.write_str(", none left at the stop")?;
        } else {
            let frames = self.after_stop as f64;
            let seconds = self.since_stop.as_secs_f64();
            let canvas = f64::from(width) * f64::from(height);
            write!(
                f,
                ", {} of them after the stop in {seconds:.1} s ({:.1} a second); \
                 preparing took {:.1} ms a frame on {} threads, busy {:.0}% of that time, \
                 and writing {:.1} ms a frame, busy {:.0}%; \
                 the changed area averaged {:.0}% of a frame",
                self.after_stop,
                ratio(frames, seconds),
                ratio(self.preparing.as_secs_f64() * 1000.0, frames),
                self.threads,
                ratio(
                    self.preparing_since.as_secs_f64() * 100.0,
                    self.threads as f64 * seconds
                ),
                ratio(self.writing.as_secs_f64() * 1000.0, frames),
                ratio(self.writing_since.as_secs_f64() * 100.0, seconds),
                ratio(self.changed as f64 * 100.0, frames * canvas),
            )?;
        }
        write!(
            f,
            "; closing the file took {:.1} s",
            self.closing.as_secs_f64()
        )
    }
}

fn ratio(part: f64, whole: f64) -> f64 {
    if whole > 0.0 { part / whole } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn span(stop: Instant, from: i64, to: i64) -> Span {
        let at = |millis: i64| {
            let offset = ms(millis.unsigned_abs());
            if millis < 0 {
                stop.checked_sub(offset).unwrap()
            } else {
                stop + offset
            }
        };
        Span {
            start: at(from),
            end: at(to),
        }
    }

    #[test]
    fn frames_written_after_the_stop_are_timed_and_the_rest_only_counted() {
        let stop = Instant::now() + Duration::from_secs(10);
        let mut pace = Pace::new(3, (1920, 1080));
        pace.wrote(
            span(stop, -1000, -600),
            span(stop, -600, -595),
            1920 * 1080,
            Some(stop),
        );
        for _ in 0..1000 {
            pace.wrote(
                span(stop, 0, 270),
                span(stop, 270, 290),
                1920 * 1080 / 2,
                Some(stop),
            );
        }
        pace.ended(Some(stop), stop + Duration::from_secs(100));
        pace.closed_in(ms(300));
        assert_eq!(
            pace.to_string(),
            "the GIF was written: 1001 frames of 1920x1080, 1000 of them after the stop in \
             100.0 s (10.0 a second); preparing took 270.0 ms a frame on 3 threads, busy 90% \
             of that time, and writing 20.0 ms a frame, busy 20%; the changed area averaged \
             50% of a frame; closing the file took 0.3 s"
        );
    }

    #[test]
    fn work_done_before_the_stop_is_left_out_of_how_busy_the_threads_were_after_it() {
        let stop = Instant::now() + Duration::from_secs(10);
        let mut pace = Pace::new(2, (100, 100));
        pace.wrote(span(stop, -2000, -1000), span(stop, -10, 10), 0, Some(stop));
        pace.wrote(
            span(stop, -500, 500),
            span(stop, 500, 520),
            100 * 100,
            Some(stop),
        );
        pace.ended(Some(stop), stop + Duration::from_secs(1));
        assert_eq!(
            pace.to_string(),
            "the GIF was written: 2 frames of 100x100, 2 of them after the stop in 1.0 s \
             (2.0 a second); preparing took 1000.0 ms a frame on 2 threads, busy 25% of that \
             time, and writing 20.0 ms a frame, busy 3%; the changed area averaged 50% of a \
             frame; closing the file took 0.0 s"
        );
    }

    #[test]
    fn a_gif_that_kept_up_has_nothing_left_at_the_stop() {
        let stop = Instant::now() + Duration::from_secs(10);
        let mut pace = Pace::new(2, (640, 360));
        pace.wrote(span(stop, -90, -60), span(stop, -60, -58), 640 * 360, None);
        pace.wrote(
            span(stop, -60, -30),
            span(stop, -30, 0),
            640 * 360,
            Some(stop),
        );
        pace.ended(Some(stop), stop);
        pace.closed_in(ms(120));
        assert_eq!(
            pace.to_string(),
            "the GIF was written: 2 frames of 640x360, none left at the stop; \
             closing the file took 0.1 s"
        );
    }

    #[test]
    fn a_stop_with_no_time_measured_divides_by_nothing() {
        let stop = Instant::now() + Duration::from_secs(10);
        let mut pace = Pace::new(0, (0, 0));
        pace.wrote(span(stop, 0, 10), span(stop, 10, 11), 0, Some(stop));
        pace.ended(Some(stop), stop);
        assert_eq!(
            pace.to_string(),
            "the GIF was written: 1 frames of 0x0, 1 of them after the stop in 0.0 s \
             (0.0 a second); preparing took 10.0 ms a frame on 0 threads, busy 0% of that \
             time, and writing 1.0 ms a frame, busy 0%; the changed area averaged 0% of a \
             frame; closing the file took 0.0 s"
        );
    }
}
