//! One reading of the selection's text at a time: what the running one was asked for, and the
//! ask made while it ran, which is read once it ends.

pub(crate) struct Turns<A> {
    started: u64,
    running: Option<Running<A>>,
}

struct Running<A> {
    request: u64,
    asked: Option<A>,
    ticket: u64,
    again: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Next<A> {
    Use { asked: A, ticket: u64 },
    ReadAgain { ticket: u64 },
    Drop,
}

impl<A> Default for Turns<A> {
    fn default() -> Self {
        Self {
            started: 0,
            running: None,
        }
    }
}

impl<A> Turns<A> {
    pub(crate) fn defer(&mut self, ticket: u64) -> bool {
        let Some(running) = self.running.as_mut() else {
            return false;
        };
        running.again = Some(ticket);
        true
    }

    pub(crate) fn request(&mut self) -> u64 {
        self.started = self.started.wrapping_add(1);
        self.started
    }

    pub(crate) fn run(&mut self, request: u64, asked: A, ticket: u64) {
        self.running = Some(Running {
            request,
            asked: Some(asked),
            ticket,
            again: None,
        });
    }

    pub(crate) fn forget(&mut self) {
        if let Some(running) = self.running.as_mut() {
            running.asked = None;
            running.again = None;
        }
    }

    pub(crate) fn finish(
        &mut self,
        request: u64,
        still_shown: impl FnOnce(&A) -> bool,
    ) -> Option<Next<A>> {
        let running = self.running.take_if(|running| running.request == request)?;
        Some(match (running.asked.filter(still_shown), running.again) {
            (Some(asked), again) => Next::Use {
                asked,
                ticket: again.unwrap_or(running.ticket),
            },
            (None, Some(ticket)) => Next::ReadAgain { ticket },
            (None, None) => Next::Drop,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_reading_runs_at_a_time_and_its_text_goes_where_the_last_ask_would() {
        let mut turns = Turns::default();
        assert!(!turns.defer(1));
        let request = turns.request();
        turns.run(request, "kept", 1);
        assert!(turns.defer(2));
        assert!(turns.defer(3));
        assert_eq!(
            turns.finish(request, |_| true),
            Some(Next::Use {
                asked: "kept",
                ticket: 3
            })
        );
        assert!(!turns.defer(4));
    }

    #[test]
    fn a_reading_of_a_changed_selection_is_read_again_if_asked_meanwhile_or_else_dropped() {
        let mut turns = Turns::default();
        let first = turns.request();
        turns.run(first, "changed", 1);
        assert!(turns.defer(2));
        assert_eq!(
            turns.finish(first, |_| false),
            Some(Next::ReadAgain { ticket: 2 })
        );
        let second = turns.request();
        assert_ne!(second, first);
        turns.run(second, "changed", 2);
        assert_eq!(turns.finish(second, |_| false), Some(Next::Drop));
    }

    #[test]
    fn a_reading_outlives_its_overlay_but_not_what_was_asked_in_it() {
        let mut turns = Turns::default();
        let request = turns.request();
        turns.run(request, "closed", 1);
        assert!(turns.defer(2));
        turns.forget();
        assert_eq!(turns.finish(request, |_| true), Some(Next::Drop));
        let request = turns.request();
        turns.run(request, "closed", 3);
        turns.forget();
        assert!(turns.defer(4));
        assert_eq!(
            turns.finish(request, |_| true),
            Some(Next::ReadAgain { ticket: 4 })
        );
    }

    #[test]
    fn only_the_running_request_ends_its_turn() {
        let mut turns = Turns::default();
        let request = turns.request();
        turns.run(request, "running", 1);
        assert_eq!(turns.finish(request + 1, |_| true), None);
        assert!(turns.defer(2));
        assert_eq!(
            turns.finish(request, |_| true),
            Some(Next::Use {
                asked: "running",
                ticket: 2
            })
        );
        assert_eq!(turns.finish(request, |_| true), None);
    }

    #[test]
    fn a_reading_that_could_not_start_leaves_the_turn_free() {
        let mut turns: Turns<&str> = Turns::default();
        let request = turns.request();
        assert!(!turns.defer(1));
        assert_eq!(turns.finish(request, |_| true), None);
    }
}
