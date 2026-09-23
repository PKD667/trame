//! An exclusive value, reached only inside one call.

use core::cell::UnsafeCell;

use super::atomic::{
    AtomicU32, Ordering::Acquire, Ordering::Relaxed, Ordering::Release as ReleaseOrdering,
};
use super::{NoUninit, from_lane_zero, lane_zero, one_lane};

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
// sharing moves `T` between warps and needs `T: Send`.
unsafe impl<T: Send> Sync for Exclusive<T> {}

impl<T> Exclusive<T> {
    pub fn new(value: T) -> Self {
        Exclusive {
            state: AtomicU32::new(FREE),
            value: UnsafeCell::new(value),
        }
    }

    /// One attempt; a held value is `Busy`, never waited for.
    pub fn with<R: NoUninit>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, Locked> {
        let old = one_lane(
            || match self.state.compare_exchange(FREE, HELD, Acquire, Relaxed) {
                Ok(old) | Err(old) => old,
            },
        );
        match old {
            FREE => {}
            ABANDONED => return Err(Locked::Abandoned),
            _ => return Err(Locked::Busy),
        }
        let result = if lane_zero() {
            let release = Release(&self.state);
            // SAFETY: HELD admits one caller.
            let result = f(unsafe { &mut *self.value.get() });
            drop(release);
            Some(result)
        } else {
            None
        };
        Ok(from_lane_zero(result))
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
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
