//! The loops of a step, as vocabulary: each carries its own termination proof in its type.
//!
//! A step is not ordinary Rust that a checker filters: it is written in a small language whose loops are
//! these combinators and `for` over a range or a slice. `drain` and `fuel` take their bound as a const
//! parameter `N`, so the number of iterations is a number in the program text, fixed before it runs. That is
//! the whole proof: the body runs at most `N` times, whatever the body does and whatever the environment does,
//! so the loop is a LOOP program (INTERFACE.md, Definition (Counted loop)) without the checker having to see it.
//! The bodies of these functions are trame's own and trusted; the closure a step passes is a body of the step
//! and is judged at the step's level like any other.
//!
//! Why a bound in the call and not a promise from `recv`: nothing in the spec stops a fast sender from refilling
//! a route as fast as a receiver empties it (INTERFACE.md 5.2, Recv and Release are independent), so
//! `while let Some(m) = recv()` has no bound that the receiver can know. `drain::<M, N>` gives the receiver one,
//! and hands the rest to the scheduler: `Drained::status` is `Progress` when the bound stopped it, so the step
//! runs again, and `wait().frame()` when the route ran dry.

use crate::io::{Message, recv};
use crate::status::Status;
use core::ops::ControlFlow;

/// What a `drain` did: how many messages it handed to the body, and whether the bound, not the route, ended it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drained {
    /// Messages handed to the body: at most `N`.
    pub taken: usize,
    /// The bound `N` was reached, so more may be waiting. False: `recv` answered none first.
    pub capped: bool,
}

impl Drained {
    /// The status that follows from it. Capped: more may wait, so the step runs again (`Progress`). Not capped: the
    /// route is dry, so the step waits for a frame (`wait().frame()`). A step that does other work says so itself.
    pub fn status(self) -> Status {
        if self.capped { Status::Progress } else { Status::wait().frame() }
    }
}

/// Take at most `N` messages of type `M` addressed to this step, in `recv` order, and give each to `body`.
/// Stops early when `recv` answers none. It never takes the `N + 1`st message, so a sender that refills the route
/// as fast as it is emptied cannot keep the step in this call.
pub fn drain<M: Message, const N: usize>(body: impl FnMut(M)) -> Drained {
    drain_from::<M, N>(recv::<M>, body)
}

/// `drain` over any source of messages: the whole logic, separated from `recv` so it is tested with a source that
/// never runs dry.
pub(crate) fn drain_from<M, const N: usize>(mut source: impl FnMut() -> Option<M>, mut body: impl FnMut(M)) -> Drained {
    for taken in 0..N {
        match source() {
            Some(m) => body(m),
            None => return Drained { taken, capped: false },
        }
    }
    Drained { taken: N, capped: true }
}

/// The fuel of a `fuel` loop ran out before the body said stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spent;

/// A `while` with its bound written down: run `body` until it answers `Break`, at most `N` times. `Ok(n)` is the
/// number of runs, the last one being the `Break`. `Err(Spent)` is `N` runs without a `Break`: the caller decides
/// whether that is an abort (`.expect(..)`) or a step to run again (`Status::Progress`).
///
/// `while cond { work }` is `fuel::<N>(|| if cond { work; Continue(()) } else { Break(()) })`.
pub fn fuel<const N: usize>(mut body: impl FnMut() -> ControlFlow<()>) -> Result<usize, Spent> {
    for run in 1..=N {
        if body().is_break() {
            return Ok(run);
        }
    }
    Err(Spent)
}

/// Lend the value in an `Option` slot of `state` to `f`, which also gets `&mut state`: the slot is emptied for the call and
/// refilled after it, so `f` can use the rest of the state while it works on the value. `slot` names the field twice (it is
/// called before and after). Answers `None`, without calling `f`, when the slot was empty.
///
/// This is the one way for a step to take a value that owns buffers out of its state and put it back. Written by hand,
/// `self.w = Some(w)` drops the old value first, and drop glue names `Vec::drop` whatever the old value is: the checker
/// judges the glue, not the value. Here the slot is known empty (asserted) and refilled with `ptr::write`, which drops nothing.
/// If `f` fills the slot itself, that is a bug and panics. If `f` panics, the value is dropped on the unwind path.
#[track_caller]
pub fn lend<S, T, R>(
    state: &mut S,
    slot: impl Fn(&mut S) -> &mut Option<T>,
    f: impl FnOnce(&mut S, &mut T) -> R,
) -> Option<R> {
    let place = slot(state);
    // Moved out by `ptr::read`, and the slot set to `None` by `ptr::write`: no `match` on an owned `Option<T>`, whose
    // not-taken arm keeps a drop of the whole value in the MIR, which the checker would judge.
    let inner: *mut T = place.as_mut()?;
    // SAFETY: `inner` points at the value in `place`, valid and aligned. The write of `None` right after overwrites the
    // slot without dropping, so the value is owned by exactly one place: `value`.
    let mut value = unsafe { core::ptr::read(inner) };
    unsafe { core::ptr::write(place, None) };
    let result = f(state, &mut value);
    let place = slot(state);
    assert!(place.is_none(), "trame: lend: the slot was filled while its value was lent");
    // SAFETY: `place` is a valid, aligned `&mut`, and holds `None` (asserted), which owns nothing: skipping the drop of
    // the old value leaks nothing. This is the one place that drop is skipped, and the reason `lend` exists.
    unsafe { core::ptr::write(place, Some(value)) };
    Some(result)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn drain_never_takes_more_than_n_from_an_endless_source() {
        let (mut offered, mut handled) = (0u64, 0u64);
        let d = drain_from::<u64, 64>(
            || {
                offered += 1;
                Some(offered)
            },
            |m| handled += m,
        );
        assert_eq!(d, Drained { taken: 64, capped: true });
        assert_eq!(offered, 64, "the 65th message is never asked for");
        assert_eq!(handled, (1..=64).sum::<u64>());
    }

    #[test]
    fn drain_stops_when_the_source_runs_dry() {
        let mut left = 5;
        let d = drain_from::<u8, 64>(
            || {
                left -= 1;
                (left >= 0).then_some(left as u8)
            },
            |_| {},
        );
        assert_eq!(d, Drained { taken: 5, capped: false });
        let none = drain_from::<u8, 64>(|| None, |_| panic!("no message"));
        assert_eq!(none, Drained { taken: 0, capped: false });
    }

    #[test]
    fn drain_of_exactly_n_is_capped() {
        // Indistinguishable from "more waits": the bound is reached, so the answer is the conservative one.
        let mut left = 3;
        let d = drain_from::<u8, 3>(
            || {
                left -= 1;
                (left >= 0).then_some(0)
            },
            |_| {},
        );
        assert_eq!(d, Drained { taken: 3, capped: true });
        assert_eq!(drain_from::<u8, 0>(|| Some(1), |_| panic!("N = 0")), Drained { taken: 0, capped: true });
    }

    #[test]
    fn status_follows_the_cap() {
        assert_eq!(Drained { taken: 3, capped: true }.status(), Status::Progress);
        assert_eq!(Drained { taken: 0, capped: false }.status(), Status::wait().frame());
    }

    #[test]
    fn fuel_counts_runs_and_runs_out() {
        let mut n = 0;
        assert_eq!(
            fuel::<10>(|| {
                n += 1;
                if n == 4 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
            }),
            Ok(4)
        );
        let mut runs = 0;
        assert_eq!(
            fuel::<7>(|| {
                runs += 1;
                ControlFlow::Continue(())
            }),
            Err(Spent)
        );
        assert_eq!(runs, 7, "an endless body is stopped after exactly N runs");
        assert_eq!(fuel::<0>(|| panic!("N = 0")), Err(Spent));
        assert_eq!(fuel::<1>(|| ControlFlow::Break(())), Ok(1));
    }

    struct Holder {
        slot: Option<u32>,
        log: u32,
    }

    #[test]
    fn lend_gives_the_value_and_the_rest_of_the_state_then_refills() {
        let mut h = Holder { slot: Some(5), log: 0 };
        let r = lend(
            &mut h,
            |h| &mut h.slot,
            |h, v| {
                assert!(h.slot.is_none(), "the slot is empty during the call");
                h.log += *v;
                *v += 1;
                "ok"
            },
        );
        assert_eq!(r, Some("ok"));
        assert_eq!((h.slot, h.log), (Some(6), 5));
    }

    #[test]
    fn lend_of_an_empty_slot_does_not_call() {
        let mut h = Holder { slot: None, log: 0 };
        assert_eq!(lend(&mut h, |h| &mut h.slot, |_, _| panic!("called")), None::<()>);
        assert_eq!(h.slot, None);
    }

    #[test]
    fn lend_panics_when_the_slot_was_filled_meanwhile() {
        let mut h = Holder { slot: Some(1), log: 0 };
        let r = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
            lend(&mut h, |h| &mut h.slot, |h, _| h.slot = Some(9))
        }));
        assert!(r.is_err());
    }
}
