//! The device's clock: a monotonic count and the identity it is comparable within.
//!
//! Two halves, one interface, chosen at compile time as `warp` and `peers` are. The host model
//! reads the process's monotonic clock; the device reads the GPU's free-running nanosecond
//! counter. Neither promises agreement with anything outside its own context, and there is no
//! realtime clock and no name for one: a program that needs wall time asks its host rather than
//! this backend.
//!
//! The incarnation is what makes two contexts' readings incomparable, and it is deliberately not
//! a rank: two runs on one rank are different clocks, and a rank says where a participant sits in
//! a cohort rather than which readings may be subtracted.

#[cfg(feature = "cuda")]
mod cuda;
#[cfg(not(feature = "cuda"))]
mod sim;

#[cfg(feature = "cuda")]
use cuda::{incarnation, nanos};
#[cfg(not(feature = "cuda"))]
use sim::{incarnation, nanos};

use crate::contract::{ClockId, Reading, Span};

/// The reading takes no participant: a clock is a property of the machine, and this one is per
/// device power cycle, which is what the incarnation says.
pub fn reading() -> Reading {
    Reading {
        clock: ClockId {
            incarnation: incarnation(),
        },
        elapsed: Span::from_nanos(nanos()),
    }
}
