//! The host model's clock: the process's monotonic origin.
//!
//! One origin per process, taken once, so two readings taken far apart differ by the span between
//! them. The incarnation mixes the process id into the origin, which is enough to keep two live
//! processes from comparing readings; it is not a name and is not promised unique forever.

use std::sync::OnceLock;
use std::time::Instant;

fn origin() -> Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(Instant::now)
}

/// Nanoseconds since this process's origin.
pub fn nanos() -> u64 {
    origin().elapsed().as_nanos() as u64
}

/// The identity two readings must share before they may be subtracted.
///
/// Taken once. An identity recomputed per call would differ between two readings of one clock, so
/// every pair would be incomparable — the opposite of what it is for, and a failure that only
/// shows up when two readings are actually compared.
pub fn incarnation() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| std::process::id() ^ (origin().elapsed().as_nanos() as u32))
}
