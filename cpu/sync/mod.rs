// CPU implementations of the three shared-information primitive families.
//
//   A  `turn`     one mutable owner at a time, handed on by registered priority
//   B  `publish`  one writer drafting privately, one reader taking coherent versions
//      `handoff`  bounded ownership transfer that never drops what it accepted
//   C  `atomic`   the conventional atomics, re-exported with their orderings
//
// These are generic primitives: no model type, no scheduler, no registry, no runtime backend
// choice, and no assumption about how many streams an application runs. They are built on
// `std::sync::Mutex` and thread parking, so they exist in every build including the one with no
// transport. What they cost is written down where it is paid; none of it is claimed to be free
// except the private field writes, which are plain stores.
//
// Waiting is by `thread::park`, never by spinning: a caller that cannot proceed sleeps and is
// woken by whoever changed its condition. Park tokens are sticky, so a wake that arrives before
// the sleep still counts, and every wait is a loop because a park may return spuriously. A caller
// that parks for its own reasons must already tolerate that (`std::thread::park`).
//
// Which calls wait, and on what, is part of each primitive's contract rather than an accident of
// this file. A call documented as not waiting takes the metadata lock with `if_free` and reports
// the pressure instead of parking behind a descheduled peer; a call that may wait takes it with
// `metadata`. Both are stated at every method.

pub mod atomic;
pub mod channel;
pub mod handoff;
pub mod publish;
pub mod turn;

#[cfg(test)]
mod tests;

// `Arc`, `Mutex`, `RwLock`, `OnceLock` and their guards are deliberately **not** re-exported here.
// They are host allocation and host synchronisation, and a name in this module is a claim that the
// backend provides a mechanism for it: a mutex blocks a host thread and an `Arc` counts host
// allocations, so a backend whose participants are not host threads could not answer either. The
// caller that needs one does not need permission — host code is ordinary Rust — so the honest
// place for these names is `std::sync` at the call site. Re-exporting them made a std facility
// look like a backend capability, which is the same mistake as a declaration boolean.
//
// The mutex the primitives are built on is still one name, privately: through `rt`, so that the
// mutex `Exclusive` uses and the mutex this module names are the same object — `std`'s in every
// ordinary build, the interleaving model's under `--cfg loom`, which is a test build and never a
// shipped one.
use rt::{Mutex, MutexGuard};
// Private, and `std`'s in every build: it is the metadata lock's error type, and the lock is
// reached through `rt` only so that the primitive and this module name the same object.
use std::sync::TryLockError;

// What stays is the three families. `turn::Exclusive` is not a `Mutex` substitute and is not
// offered as one: a caller that needs many concurrent readers says `RwLock` itself rather than
// having this module serialise them under a name that promised otherwise.

pub use handoff::{Idle, Receiver, Refused, Sender, Unsent, handoff, stocked};
pub use publish::{Ended, Pinned, Pressure, Reader, Version, Writer, published};
// The bounded waiter table's default capacity. Hoisted out of the family because it is a
// number an application states, not a path into a module: a caller that says how many waiters it
// has is declaring a bound, and reading it from `turn::` made it look like an implementation
// detail of one primitive.
pub use turn::{Exclusive, NORMAL, Priority, Turn, WAITERS};

use rt::{AtomicBool, SeqCst, Thread, ThreadId, thread};

/// The primitives under test with `loom` use its threads, mutexes and atomics; every other build
/// uses the real ones. Nothing outside this module sees the difference.
#[cfg(not(loom))]
pub(crate) mod rt {
    pub(crate) use std::sync::atomic::AtomicBool;
    pub(crate) use std::sync::atomic::Ordering::SeqCst;
    pub use std::sync::{Mutex, MutexGuard};
    pub(crate) use std::thread::{self, Thread, ThreadId};

    pub(crate) use std::sync::Arc;
}

#[cfg(loom)]
pub(crate) mod rt {
    pub(crate) use loom::sync::atomic::AtomicBool;
    pub(crate) use loom::sync::atomic::Ordering::SeqCst;
    pub use loom::sync::{Mutex, MutexGuard};
    pub(crate) use loom::thread::{self, Thread, ThreadId};

    pub(crate) use loom::sync::Arc;
}

/// Threads one `Cancel` makes room for, when the caller states no number of its own.
///
/// A generic default, not a participant count: nothing here knows how many streams an
/// application runs, and nothing here decides. The application knows its participants and
/// either passes them to `Cancel::with_capacity` or checks that this default covers them.
/// Overflowing the table is refused explicitly rather than silently grown.
pub const WATCHERS: usize = 4;

/// A waiting call ended because someone asked it to, not because it got what it waited for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Cancelled;

/// A one-way shutdown request shared by the callers that must stop waiting when it is raised.
///
/// Raising is level-triggered and permanent: once raised, every present and future wait against
/// this token ends at its next check. That is what reset and teardown need, and it is why
/// there is no `lower()`: a token that can be lowered turns a missed edge into a lost wake-up.
///
/// Waiting callers register the thread they park on, so raising wakes them immediately rather
/// than at some poll interval. The table is bounded: a caller that would overflow it panics
/// rather than allocating. How many callers can wait on one token is a fact the application
/// states at construction, not one this module discovers by growing a queue on a hot path.
pub struct Cancel {
    raised: AtomicBool,
    /// Waiting callers this token makes room for. `Vec` may hand out more capacity than asked
    /// for; the limit is the number that was asked for, so the refusal is where it is documented.
    limit: usize,
    /// Threads parked on a wait that names this token.
    watching: Mutex<Vec<Thread>>,
}

impl Default for Cancel {
    fn default() -> Self {
        Self::new()
    }
}

impl Cancel {
    /// Room for `WATCHERS` concurrent waiting callers.
    pub fn new() -> Self {
        Self::with_capacity(WATCHERS)
    }

    /// Room for `watchers` concurrent waiting callers.
    pub fn with_capacity(watchers: usize) -> Self {
        Cancel {
            raised: AtomicBool::new(false),
            limit: watchers,
            watching: Mutex::new(Vec::with_capacity(watchers)),
        }
    }

    /// Ask every waiting caller to stop. Permanent, and safe to call more than once.
    pub fn raise(&self) {
        // Ordered before the table is taken, which is what makes a caller that registers later
        // see the raised flag: its registration acquires the same lock this release-stores into.
        self.raised.store(true, SeqCst);
        let watching = metadata(&self.watching);
        for thread in watching.iter() {
            thread.unpark();
        }
    }

    /// Whether the request has been raised.
    pub fn raised(&self) -> bool {
        self.raised.load(SeqCst)
    }

    /// Register this thread for the duration of a wait.
    ///
    /// Registration happens before the first check of `raised`, so `raise` either sees this
    /// thread in the table and unparks it, or happened before the registration and is therefore
    /// visible to the check that follows it. A park token delivered before the caller parks is
    /// not lost, so no wake-up can slip between the two.
    ///
    /// This call is also where a wait can be refused, so a caller registers here *before* it
    /// takes any ticket of its own: a refusal then finds the caller owning nothing, and leaves
    /// no registration behind for a resource to hand itself to.
    ///
    /// # Panics
    ///
    /// When more callers wait on one token than its capacity allows.
    pub(crate) fn watch(&self) -> Watching<'_> {
        let mut watching = metadata(&self.watching);
        if watching.len() == self.limit {
            drop(watching);
            panic!("cancel: more than {} waiting callers", self.limit);
        }
        let thread = thread::current();
        let id = thread.id();
        watching.push(thread);
        drop(watching);
        Watching { cancel: self, id }
    }
}

/// Deregisters the waiting thread when the wait ends, however it ends.
pub(crate) struct Watching<'a> {
    cancel: &'a Cancel,
    id: ThreadId,
}

impl Drop for Watching<'_> {
    fn drop(&mut self) {
        let mut watching = metadata(&self.cancel.watching);
        if let Some(at) = watching.iter().position(|thread| thread.id() == self.id) {
            watching.swap_remove(at);
        }
    }
}

/// Take a metadata lock, waiting for it. For the calls whose contract says they may wait, and
/// for construction and teardown, where waiting is disclosed at the call site.
pub(crate) fn metadata<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().expect(POISONED)
}

/// Take a metadata lock if it is free this instant, and never wait for it.
///
/// `None` says only that the peer held the lock at that moment: it says nothing about the state
/// the lock protects. That is the distinction a nonblocking caller needs, because the peer may
/// be descheduled while holding it, and "come back" is then a correct answer where "wait" is not.
pub(crate) fn if_free<T>(lock: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match lock.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::WouldBlock) => None,
        Err(TryLockError::Poisoned(_)) => panic!("{POISONED}"),
    }
}

/// Metadata locks are held only across the short bookkeeping in this module, which cannot panic,
/// so a poisoned one means the process is already failing.
pub(crate) const POISONED: &str = "sync: metadata lock poisoned";
