// The types the transport hands to its callers: route ids, answers, roles, errors and trace events.

extern crate std;

use std::fmt;

use super::Transport;

/// A route, by its index in the topology. The worker or leader that sends or receives it names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteId(pub u32);

/// The answer to a send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Ok,
    Full,
}

/// What a rank is in a launch.
pub enum Role {
    Worker(Transport),
    Leader(Transport),
}

/// The answer to a receive: a message was written to the buffer, or nothing was available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Received {
    Message(RouteId),
    Empty,
}

/// A failure the program must handle. None of these is a Full or an Empty, which are answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The barrier can never complete: a worker returned while others wait at it (spec.md 5,
    /// Deadlock). The waiter ends with `Outcome::Deadlock`.
    Deadlock,
    /// The run aborted while this worker waited at the barrier. The waiter unwinds; its run is aborted.
    Aborted,
    /// The endpoint sends on a route it does not send on, or receives on one it does not receive.
    NotYours(RouteId),
    /// The message is not the route's size, or the buffer is shorter than it.
    BadSize { route: RouteId, expected: usize, got: usize },
    /// A route id outside the topology.
    NoSuchRoute(RouteId),
    /// A ring invariant failed. This is a bug in the transport, reported instead of hidden.
    Broken(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Deadlock => write!(f, "barrier can never complete: a worker returned"),
            Error::Aborted => write!(f, "the run aborted at a barrier"),
            Error::NotYours(r) => write!(f, "route {} is not this endpoint's", r.0),
            Error::BadSize { route, expected, got } => {
                write!(f, "route {} takes {expected} bytes, got {got}", route.0)
            }
            Error::NoSuchRoute(r) => write!(f, "no route {}", r.0),
            Error::Broken(what) => write!(f, "transport invariant broken: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// One recorded effect, when tracing is on. Times are `clock()` readings, which share one clock
/// across all ranks of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A send. `index` is P before the send, `read` is the C value the answer was linearized at
    /// (0 on lossy routes, which never read C). `before` and `after` bracket that read.
    Send { route: u32, index: u64, read: u64, before: u64, after: u64, answer: Answer },
    /// A frame consumed. `index` is its position on the route. `before` is taken before the
    /// consume is published and `after` once it is.
    Consume { route: u32, index: u64, before: u64, after: u64 },
}

/// Nanoseconds on `CLOCK_MONOTONIC`. On Linux this clock is the same for every process on a node,
/// which is what lets the test compare readings from different ranks.
pub fn clock() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    assert_eq!(rc, 0, "clock_gettime failed");
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
