// Host clock: the one reading a program can take of the machine it runs on.
//
//   `reading()`     monotonic, counted from this process's origin. Never goes backwards, has no
//                   meaning in another process, and is what a span *within* one rank is measured
//                   with.
//
// There is deliberately no realtime reading here. Wall time belongs to the host, not to a
// participant: a program that needs it asks the host it is running on, which is `std::time` and
// is host code by definition. Giving it a backend name would force every backend to either answer
// with an approximation or refuse it, and an approximated wall clock is not what a caller asking
// for one needs.
//
// The process origin is taken once, lazily, by whichever caller reads first, so every `Reading`
// in a process shares one base and stays small enough for exact arithmetic above. Reading costs
// one clock read: there is no clocks object to look up, no allocation, and no lock.
//
// What is *not* here: model time. Anchors, scales, rescaling, retiming and the meaning of a
// spike's due time belong to the application (`src/nerve/time.rs`) and this module imports none
// of it. It also owns no schedule: a deadline is a comparison the caller makes.
//
// A device backend supplies the same call: a GPU has no `SystemTime`, and
// its monotonic counter is per-device. `backend.md` §7 states what that means for a caller that
// wants to stamp work running there.

use crate::contract::{ClockId, Reading, Span};

/// The host's clock, as the portable reading.
///
/// One origin per process, taken once, so two readings taken far apart differ by the span between
/// them. The reading is the contract's [`Reading`] and not a host type of its own: a second
/// reading type would be a second thing for an application to convert between.
pub fn reading() -> Reading {
    Reading::new(
        clock_id(),
        Span::from_nanos(origin().elapsed().as_nanos() as u64),
    )
}

/// The identity two readings must share before they may be subtracted: one per process, formed
/// from the process id. Not a rank, because two runs on one rank are different clocks.
fn clock_id() -> ClockId {
    static ID: std::sync::OnceLock<ClockId> = std::sync::OnceLock::new();
    *ID.get_or_init(|| ClockId::new(std::process::id()))
}

fn origin() -> std::time::Instant {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *ORIGIN.get_or_init(std::time::Instant::now)
}
