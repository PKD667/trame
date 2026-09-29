// An exclusive value, reached only inside one call.

use core::cell::UnsafeCell;
use super::atomic::{
    AtomicU32, Ordering::Acquire, Ordering::Relaxed, Ordering::Release as ReleaseOrdering,
};

/// Why `with` did not run its closure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locked {
    /// Another call is inside: contention.
    Busy,
    /// A closure panicked inside, so `T` may be torn: every later `with` answers this.
    Abandoned,
}

const FREE: u32 = 0;
const HELD: u32 = 1;
const ABANDONED: u32 = 2;

/// A value exactly one call may mutate at a time.
pub struct Exclusive<T> {
    state: AtomicU32,
    value: UnsafeCell<T>,
}

// SAFETY: `value` is reached only inside `with`, and `state` admits one `with` at a time, so
// sharing moves `T` between execution owners and needs `T: Send`.
unsafe impl<T: Send> Sync for Exclusive<T> {}

impl<T> Exclusive<T> {
    pub fn new(value: T) -> Self {
        Exclusive {
            state: AtomicU32::new(FREE),
            value: UnsafeCell::new(value),
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
}

/// One attempt; a held value is `Busy`, never waited for.
pub fn with<T, R>(on: &Exclusive<T>, f: impl FnOnce(&mut T) -> R) -> Result<R, Locked> {
    match on.state.compare_exchange(FREE, HELD, Acquire, Relaxed) {
        Ok(_) => {}
        Err(ABANDONED) => return Err(Locked::Abandoned),
        Err(_) => return Err(Locked::Busy),
    }
    let release = Release(&on.state);
    // SAFETY: HELD admits one caller.
    let result = f(unsafe { &mut *on.value.get() });
    drop(release);
    Ok(result)
}

struct Release<'a>(&'a AtomicU32);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        #[cfg(feature = "cuda")]
        self.0.store(FREE, ReleaseOrdering);
        #[cfg(not(feature = "cuda"))]
        self.0.store(
            if std::thread::panicking() {
                ABANDONED
            } else {
                FREE
            },
            ReleaseOrdering,
        );
    }
}
