//! The atomics a step's shared state uses, with `cpu::sync::atomic`'s method set: the host model
//! wraps `core`'s words, and `cuda` the device's.
//!
//! `core` and the device number `Acquire` and `Release` differently, so orderings are mapped by
//! name, never by discriminant: a cast would compile and swap them.

pub use core::sync::atomic::Ordering;

#[cfg(not(feature = "cuda"))]
use core::sync::atomic::{AtomicU32 as Word32, AtomicU64 as Word64};
#[cfg(feature = "cuda")]
use cuda_device::atomic::{DeviceAtomicU32 as Word32, DeviceAtomicU64 as Word64};

#[cfg(not(feature = "cuda"))]
#[inline(always)]
fn order(ordering: Ordering) -> Ordering {
    ordering
}

#[cfg(feature = "cuda")]
#[inline(always)]
fn order(ordering: Ordering) -> cuda_device::atomic::AtomicOrdering {
    use cuda_device::atomic::AtomicOrdering;
    match ordering {
        Ordering::Relaxed => AtomicOrdering::Relaxed,
        Ordering::Acquire => AtomicOrdering::Acquire,
        Ordering::Release => AtomicOrdering::Release,
        Ordering::AcqRel => AtomicOrdering::AcqRel,
        Ordering::SeqCst => AtomicOrdering::SeqCst,
        // `Ordering` is `#[non_exhaustive]`; a new variant must not land on a guessed arm.
        _ => unreachable!("core::sync::atomic::Ordering has a variant this mapping does not know"),
    }
}

macro_rules! wrapped {
    ($name:ident, $word:ident, $ty:ty, $into:ident, $from:ident $(, $add:ident)?) => {
        #[repr(transparent)]
        pub struct $name($word);

        impl $name {
            #[inline(always)]
            pub const fn new(value: $ty) -> Self {
                Self($word::new($into(value)))
            }

            #[inline(always)]
            pub fn load(&self, ordering: Ordering) -> $ty {
                $from(self.0.load(order(ordering)))
            }

            #[inline(always)]
            pub fn store(&self, value: $ty, ordering: Ordering) {
                self.0.store($into(value), order(ordering))
            }

            #[inline(always)]
            pub fn swap(&self, value: $ty, ordering: Ordering) -> $ty {
                $from(self.0.swap($into(value), order(ordering)))
            }

            #[inline(always)]
            pub fn compare_exchange(
                &self,
                current: $ty,
                new: $ty,
                success: Ordering,
                failure: Ordering,
            ) -> Result<$ty, $ty> {
                self.0
                    .compare_exchange($into(current), $into(new), order(success), order(failure))
                    .map($from)
                    .map_err($from)
            }

            $(
                #[inline(always)]
                pub fn $add(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.fetch_add(value, order(ordering))
                }
            )?
        }

        impl Default for $name {
            /// Zero, or `false`, as `core`'s atomics.
            #[inline(always)]
            fn default() -> Self {
                Self::new(<$ty>::default())
            }
        }
    };
}

// A device has no boolean atomic, so both builds hold a boolean in a 0/1 word.
const fn word(value: bool) -> u32 {
    value as u32
}

const fn same32(value: u32) -> u32 {
    value
}

const fn same64(value: u64) -> u64 {
    value
}

fn set(value: u32) -> bool {
    value != 0
}

wrapped!(AtomicBool, Word32, bool, word, set);
wrapped!(AtomicU32, Word32, u32, same32, same32, fetch_add);
wrapped!(AtomicU64, Word64, u64, same64, same64, fetch_add);
