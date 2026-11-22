//! Clocks and readings: INTERFACE.md section 5.1. A reading compares only with a reading of its own
//! clock, so trame never has to synchronize clocks. Spans are `core::time::Duration`.

use crate::backend::{Backend, Selected};
use core::time::Duration;

/// Identity of a clock: the clock of a device (INTERFACE.md 5.1). Readings of two clocks never compare. A backend gives
/// the clocks of its devices the ids below `ClockId::DETACHED`; a program that needs a clock no backend owns (a
/// test) names one at or above it, with `ClockId::detached`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockId(u64);

impl ClockId {
    /// The first id no backend gives to a device.
    pub const DETACHED: u64 = 1 << 63;

    /// The clock of a device, numbered by its backend.
    pub(crate) const fn new(id: u64) -> Self {
        ClockId(id)
    }

    pub(crate) const fn raw(self) -> u64 {
        self.0
    }

    /// A clock that no device has, number `n`: for a test. Two of the same `n` are one clock.
    pub const fn detached(n: u64) -> Self {
        ClockId(Self::DETACHED | (n & !Self::DETACHED))
    }
}

/// The current reading of this worker's clock (INTERFACE.md 5.1, `now`). Panics outside a worker: a leader, and a
/// thread it started, have no trame clock and use `std::time`.
pub fn now() -> Reading {
    let (clock, nanos) = Selected::worker_now();
    Reading { clock, nanos }
}

/// A clock reading: an instant `at` on the clock `clock`. The offset of the clock is unknown, so
/// only differences between readings of one clock mean anything. A reading is plain data that sits in a state;
/// it is not a message, because a reading alone means nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reading {
    clock: ClockId,
    nanos: u64,
}

impl Reading {
    /// The reading `at` after the start of `clock`. Any value is a reading the clock could show, since its
    /// offset is arbitrary (INTERFACE.md 5.1): a test builds its times with it, and a worker never needs it.
    /// Saturates at `u64::MAX` nanoseconds (about 584 years).
    pub fn new(clock: ClockId, at: Duration) -> Reading {
        Reading { clock, nanos: u64::try_from(at.as_nanos()).unwrap_or(u64::MAX) }
    }

    /// The clock this reading is of.
    pub const fn clock(self) -> ClockId {
        self.clock
    }

    /// The instant of this reading on its clock, as the time since the clock started.
    pub(crate) const fn at(self) -> Duration {
        Duration::from_nanos(self.nanos)
    }

    /// The time from `earlier` to `self`, when both are readings of one clock, and `None` when they are of two.
    /// It is zero when `earlier` is the later of the two, so `a.since(b)` and `b.since(a)` together give the
    /// order and the signed difference: both zero is equal, and the one that is not zero is the later.
    pub fn since(self, earlier: Reading) -> Option<Duration> {
        (self.clock == earlier.clock).then(|| self.at().saturating_sub(earlier.at()))
    }
}
