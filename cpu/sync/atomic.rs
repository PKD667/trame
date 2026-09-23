// The atomics a step's shared state uses, wrapped so their method set is exactly the one the
// device answers: a call that builds here builds under `cuda`.

use core::sync::atomic;

pub use core::sync::atomic::Ordering;

macro_rules! wrapped {
    ($name:ident, $ty:ty $(, $add:ident)?) => {
        #[repr(transparent)]
        pub struct $name(atomic::$name);

        impl $name {
            #[inline(always)]
            pub const fn new(value: $ty) -> Self {
                Self(atomic::$name::new(value))
            }

            #[inline(always)]
            pub fn load(&self, ordering: Ordering) -> $ty {
                self.0.load(ordering)
            }

            #[inline(always)]
            pub fn store(&self, value: $ty, ordering: Ordering) {
                self.0.store(value, ordering)
            }

            #[inline(always)]
            pub fn swap(&self, value: $ty, ordering: Ordering) -> $ty {
                self.0.swap(value, ordering)
            }

            #[inline(always)]
            pub fn compare_exchange(
                &self,
                current: $ty,
                new: $ty,
                success: Ordering,
                failure: Ordering,
            ) -> Result<$ty, $ty> {
                self.0.compare_exchange(current, new, success, failure)
            }

            $(
                #[inline(always)]
                pub fn $add(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.fetch_add(value, ordering)
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

wrapped!(AtomicBool, bool);
wrapped!(AtomicU32, u32, fetch_add);
wrapped!(AtomicU64, u64, fetch_add);
