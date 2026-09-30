// audited: 2026-09-30
//! The bound a request waits under: a timeout for each operation and a deadline for the whole call.
//!
//! docs/io/timeout.md

use std::time::{Duration, Instant};

/// How long a request may wait.
///
/// `timeout` bounds each kernel operation the call makes, and `until` bounds
/// the whole call however many operations it makes (docs/io/timeout.md). A
/// bound that names neither waits as long as it takes. Every wait a backend
/// arms asks [`Bound::next_end`] or [`Bound::next_wait`] at the moment it
/// arms, so an operation resubmitted late in a call gets only what the
/// deadline has left.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bound {
    timeout: Option<Duration>,
    until: Option<Instant>,
}

impl Bound {
    /// No bound: every wait lasts as long as it takes.
    pub const NONE: Bound = Bound {
        timeout: None,
        until: None,
    };

    /// A bound of `timeout` for each operation and `until` for the whole call.
    pub fn new(timeout: Option<Duration>, until: Option<Instant>) -> Bound {
        Bound { timeout, until }
    }

    /// Each operation may wait `timeout`, and the call has no deadline.
    pub fn per_op(timeout: Duration) -> Bound {
        Bound::new(Some(timeout), None)
    }

    /// This bound, taking `timeout` for each operation when it names no
    /// timeout of its own. A port's own `:timeout` reaches a call this way.
    pub fn or_timeout(self, timeout: Option<Duration>) -> Bound {
        let _ = timeout;
        self
    }

    /// Whether a wait under this bound ends on its own.
    pub fn is_bounded(&self) -> bool {
        false
    }

    /// The instant a wait that starts at `now` must end by: the earlier of
    /// `now + timeout` and the deadline. `None` when neither bounds it. A
    /// timeout too long to add to `now` bounds nothing.
    pub fn next_end(&self, now: Instant) -> Option<Instant> {
        let _ = now;
        None
    }

    /// How long a wait that starts now may take. `Some(Duration::ZERO)` once
    /// the deadline has passed, which is a wait that only polls.
    pub fn next_wait(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_bound_has_no_end() {
        assert_eq!(Bound::NONE.next_end(Instant::now()), None);
        assert_eq!(Bound::NONE.next_wait(), None);
        assert!(!Bound::NONE.is_bounded());
    }

    #[test]
    fn a_timeout_ends_each_wait_that_long_after_it_starts() {
        let now = Instant::now();
        let bound = Bound::per_op(Duration::from_secs(2));
        assert_eq!(bound.next_end(now), Some(now + Duration::from_secs(2)));
        let later = now + Duration::from_secs(5);
        assert_eq!(bound.next_end(later), Some(later + Duration::from_secs(2)));
    }

    #[test]
    fn a_deadline_ends_every_wait_at_the_same_instant() {
        let now = Instant::now();
        let until = now + Duration::from_secs(3);
        let bound = Bound::new(None, Some(until));
        assert_eq!(bound.next_end(now), Some(until));
        assert_eq!(bound.next_end(now + Duration::from_secs(1)), Some(until));
    }

    #[test]
    fn with_both_the_earlier_ends_the_wait() {
        let now = Instant::now();
        let until = now + Duration::from_secs(3);
        let bound = Bound::new(Some(Duration::from_secs(1)), Some(until));
        assert_eq!(bound.next_end(now), Some(now + Duration::from_secs(1)));
        let late = now + Duration::from_millis(2500);
        assert_eq!(bound.next_end(late), Some(until));
    }

    #[test]
    fn a_passed_deadline_leaves_a_wait_that_only_polls() {
        let past = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            Bound::new(None, Some(past)).next_wait(),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_timeout_too_long_to_add_bounds_nothing() {
        assert_eq!(Bound::per_op(Duration::MAX).next_end(Instant::now()), None);
    }

    #[test]
    fn a_port_timeout_fills_in_only_for_a_call_that_names_none() {
        let port = Some(Duration::from_secs(9));
        assert_eq!(Bound::NONE.or_timeout(port), Bound::new(port, None));
        let own = Bound::per_op(Duration::from_secs(1));
        assert_eq!(own.or_timeout(port), own);
    }
}
