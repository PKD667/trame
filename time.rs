//! Clocks and readings: spec.md section 5.1. A reading compares only with a reading of its own
//! clock, so trame never has to synchronize clocks. Spans are `core::time::Duration`.

use crate::backend::{Backend, Selected};
use core::time::Duration;

/// Identity of a clock. The spec gives each device one clock; readings of different clocks never
/// compare. Not part of the surface: a reading exposes only its span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClockId {
    incarnation: u32,
}

impl ClockId {
    pub(crate) const fn new(incarnation: u32) -> Self {
        ClockId { incarnation }
    }
}

/// The current reading of this worker's clock (spec.md 5.1, `now`). Panics outside a worker.
pub fn now() -> Reading {
    let (clock, nanos) = Selected::worker_now();
    Reading { clock, at: Duration::from_nanos(nanos) }
}

/// A clock reading: an instant `at` on the clock `clock`. The offset of the clock is unknown, so
/// only differences between readings of one clock mean anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    clock: ClockId,
    at: Duration,
}

impl Reading {
    /// The clock this reading is of.
    pub(crate) const fn clock(self) -> ClockId {
        self.clock
    }

    /// The instant of this reading on its clock, as the time since the clock started.
    pub(crate) const fn at(self) -> Duration {
        self.at
    }

    /// The instant `span` after `self`, on the same clock: the deadline of a `.until`. Overflow
    /// panics, since a silently wrapped deadline would wake the process at the wrong time.
    pub fn after(self, span: Duration) -> Reading {
        let at = self.at.checked_add(span).expect("trame: a deadline overflows the clock");
        Reading { clock: self.clock, at }
    }

    /// The time from `earlier` to `self`. Aborts when the readings come from different clocks,
    /// which the spec leaves undefined: a program that compares them is wrong, and must not go on.
    pub fn since(self, earlier: Reading) -> Duration {
        assert!(
            self.clock == earlier.clock,
            "trame: Reading::since was given readings of different clocks; only readings of one clock compare"
        );
        self.at.saturating_sub(earlier.at)
    }
}
