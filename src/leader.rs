//! The leader's waiting, in one place: one policy, one sleep, and the rule that says who may use it.
//!
//! Two kinds of agent wait in trame, and they wait differently. A worker polls its incoming messages and never
//! sleeps: its latency is the run's latency. A leader polls the peer channel — the leader-to-leader channel, which is
//! not a route — and sleeps between polls, because a leader that spins holds a core for the whole wait and, worse,
//! stays out of its run loop, so the routes into it fill and its own workers starve. This module holds that sleep and
//! the one bound that goes with it, so the rule is a name instead of a comment repeated in two backends.
//!
//! LEADER TO LEADER ONLY. Nothing else in trame sleeps on this policy:
//!
//! - a relayed route frame travels on the peer channel too (`transport/cross`, F5), but a relay moves frames in its
//!   own loop and calls no wait, so a forwarded message never pays [POLL];
//! - a worker never sleeps at all;
//! - a leader that waits for its own workers naps on its epoch futex instead ([NAP_NANOS] bounds that nap), so a
//!   worker's push wakes it at once.
//!
//! A backend without peers uses none of this module, which is why its items carry a `dead_code` allowance: they are
//! used by the backends that have a peer channel, and the lints of a build without one have nothing to say about it.
//!
//! A change that puts [POLL] or [sleep] on a worker's path, or on a relay's, is a latency regression on every route.

extern crate std;

use core::time::Duration;

/// The interval between two polls of the peer channel: 100 µs. It bounds how late a leader notices a peer message,
/// and it is the smallest interval that leaves the core idle. A worker's push does not wait for it.
#[allow(dead_code)]
pub(crate) const POLL: Duration = Duration::from_micros(100);

/// `POLL` in the nanoseconds the epoch protocol takes.
#[allow(dead_code)]
pub(crate) const NAP_NANOS: u64 = 100_000;

/// Sleep between two polls: the leader's own wait, when there is no futex to be woken on (the peer channel belongs to
/// the message library, which offers no descriptor to wait on).
#[allow(dead_code)]
pub(crate) fn sleep() {
    std::thread::sleep(POLL);
}
