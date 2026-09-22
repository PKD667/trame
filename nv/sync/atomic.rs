//! Family C on the device: the conventional atomic operations, under the names the host family
//! uses.
//!
//! # Why this is an adapter and not a re-export
//!
//! The CPU family is `pub use std::sync::atomic::{…}` — nothing to implement, because the host
//! already has the family. The device version cannot be that, for a reason that is a name rather
//! than a gap: `cuda_device::atomic` calls its type `DeviceAtomicU32` and its ordering enum
//! `AtomicOrdering`, *deliberately*, so that device code can still write
//! `core::sync::atomic::Ordering` for its own arithmetic without a name clash. So the operations
//! are the same and the names are not, and a program that writes `sync::atomic::AtomicU32` cannot
//! be compiled against this backend until something maps one onto the other. That something is
//! this module, and it is why the family was declared unsupported until it existed: a name that
//! resolves to nothing is not a capability.
//!
//! # What the mapping costs, and the one thing it must not do
//!
//! `core`'s `Ordering` and the device's `AtomicOrdering` have the same five variants and the same
//! meaning, and **different discriminants**: `core` numbers `Release` 1 and `Acquire` 2, the
//! device numbers `Acquire` 1 and `Release` 2. So the mapping is written by name and never by
//! `transmute`. A discriminant cast would swap a release for an acquire — the one mistake in this
//! family that no test can see, because both orderings are legal and only the timing differs. It
//! is also why the mapping is a `match` over all five rather than a lookup: a sixth variant
//! arriving upstream becomes a compile error here instead of falling into a catch-all.
//!
//! # Scope
//!
//! A device atomic is device-scoped: it orders the warps of one launch, which is a declared
//! sharing domain and not one participant. That is the `domain` scope, and it is a weaker claim
//! than it sounds — nothing here orders anything against another node, and no amount of `SeqCst`
//! makes a second host observe anything.
//!
//! # Widths
//!
//! Four, and the same four the host family now offers: `u32`, `i32`, `u64`, `i64`. The device has
//! no boolean or pointer-width atomic of its own — a `usize` on this target is 64 bits, so
//! `AtomicU64` is the same machine word, but naming it `usize` would promise a width this target
//! does not have. Another width is one line in the macro below and one in the host family, added
//! by the call site that needs it.

#[cfg(not(feature = "cuda"))]
pub use core::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};

/// The same family, over the device's own atomics.
///
/// Every method takes `core::sync::atomic::Ordering`, which is the name the host family takes, so
/// a call site moves between backends without changing an ordering — and the ordering is the one
/// thing in this family a caller should never have to change as a side effect of a transport.
#[cfg(feature = "cuda")]
mod device {
    use core::sync::atomic::Ordering;

    use cuda_device::atomic::{AtomicOrdering, DeviceAtomicU32};

    /// The device's name for the same ordering.
    ///
    /// By name, never by discriminant: the two enums disagree about which of `Acquire` and
    /// `Release` is 1. A cast would compile and would swap them.
    ///
    /// `core::sync::atomic::Ordering` is `#[non_exhaustive]`, so this cannot be `const fn` and
    /// cannot end at `SeqCst`: the compiler requires a wildcard whether or not a sixth variant
    /// exists today. The five named arms are still written out, so a variant this crate does not
    /// yet know about takes the wildcard rather than silently landing on whichever arm happened
    /// to be last — the mismap this module exists to prevent, kept as loud as the language lets
    /// it be.
    #[inline(always)]
    fn order(ordering: Ordering) -> AtomicOrdering {
        match ordering {
            Ordering::Relaxed => AtomicOrdering::Relaxed,
            Ordering::Acquire => AtomicOrdering::Acquire,
            Ordering::Release => AtomicOrdering::Release,
            Ordering::AcqRel => AtomicOrdering::AcqRel,
            Ordering::SeqCst => AtomicOrdering::SeqCst,
            _ => unreachable!(
                "core::sync::atomic::Ordering has a variant this mapping does not know"
            ),
        }
    }

    macro_rules! device_atomic {
        ($name:ident, $inner:ident, $ty:ty) => {
            #[doc = concat!("A `", stringify!($ty), "` the warps of one launch may share.")]
            #[repr(transparent)]
            pub struct $name(cuda_device::atomic::$inner);

            impl $name {
                /// A free-standing atomic, which on a device is a location in whatever memory the
                /// value lives in rather than a register.
                pub const fn new(value: $ty) -> Self {
                    Self(cuda_device::atomic::$inner::new(value))
                }

                /// A view over a plain value's storage.
                ///
                /// # Safety
                ///
                /// As `core::sync::atomic::AtomicU32::from_ptr`: aligned and valid for `'a`, and
                /// no non-atomic access to the location while the view is live. Mixing atomic and
                /// plain access to one location is undefined here exactly as it is on the host.
                #[inline(always)]
                pub const unsafe fn from_ptr<'a>(ptr: *mut $ty) -> &'a Self {
                    // SAFETY: the wrapper is `repr(transparent)` over the device type, so the
                    // caller's obligations are the device's verbatim.
                    unsafe { &*(ptr as *const Self) }
                }

                #[inline(always)]
                pub fn load(&self, ordering: Ordering) -> $ty {
                    self.0.load(order(ordering))
                }

                #[inline(always)]
                pub fn store(&self, value: $ty, ordering: Ordering) {
                    self.0.store(value, order(ordering));
                }

                #[inline(always)]
                pub fn swap(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.swap(value, order(ordering))
                }

                #[inline(always)]
                pub fn fetch_add(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.fetch_add(value, order(ordering))
                }

                #[inline(always)]
                pub fn fetch_sub(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.fetch_sub(value, order(ordering))
                }

                #[inline(always)]
                pub fn fetch_and(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.fetch_and(value, order(ordering))
                }

                #[inline(always)]
                pub fn fetch_or(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.fetch_or(value, order(ordering))
                }

                #[inline(always)]
                pub fn fetch_xor(&self, value: $ty, ordering: Ordering) -> $ty {
                    self.0.fetch_xor(value, order(ordering))
                }

                /// `Ok` with the value that was there, `Err` with what is there instead — the
                /// same two-way answer `core` gives, so a retry loop written for one compiles
                /// against the other.
                #[inline(always)]
                pub fn compare_exchange(
                    &self,
                    current: $ty,
                    new: $ty,
                    success: Ordering,
                    failure: Ordering,
                ) -> Result<$ty, $ty> {
                    self.0
                        .compare_exchange(current, new, order(success), order(failure))
                }
            }
        };
    }

    /// A boolean, over a 32-bit word with 0/1 semantics.
    ///
    /// Deliberately not `AtomicU32` under another name. A reader of a call site should see a
    /// boolean where the source says boolean, and the operations are over `bool` — a width alias
    /// would make the API stop meaning what it says, and the first person to store a `2` would
    /// find out at the far end of a data race.
    ///
    /// The word underneath is visible in one place and only one: there is no `from_ptr` over a
    /// `*mut bool`, because a device boolean is not one byte and a view over a byte would be a
    /// lie. A caller that needs a view places it over a word it owns.
    #[repr(transparent)]
    pub struct AtomicBool(DeviceAtomicU32);

    impl AtomicBool {
        pub const fn new(value: bool) -> Self {
            Self(DeviceAtomicU32::new(value as u32))
        }

        #[inline(always)]
        pub fn load(&self, ordering: Ordering) -> bool {
            self.0.load(order(ordering)) != 0
        }

        #[inline(always)]
        pub fn store(&self, value: bool, ordering: Ordering) {
            self.0.store(value as u32, order(ordering));
        }

        #[inline(always)]
        pub fn swap(&self, value: bool, ordering: Ordering) -> bool {
            self.0.swap(value as u32, order(ordering)) != 0
        }

        #[inline(always)]
        pub fn compare_exchange(
            &self,
            current: bool,
            new: bool,
            success: Ordering,
            failure: Ordering,
        ) -> Result<bool, bool> {
            self.0
                .compare_exchange(current as u32, new as u32, order(success), order(failure))
                .map(|was| was != 0)
                .map_err(|is| is != 0)
        }

        #[inline(always)]
        pub fn fetch_and(&self, value: bool, ordering: Ordering) -> bool {
            self.0.fetch_and(value as u32, order(ordering)) != 0
        }

        #[inline(always)]
        pub fn fetch_or(&self, value: bool, ordering: Ordering) -> bool {
            self.0.fetch_or(value as u32, order(ordering)) != 0
        }

        #[inline(always)]
        pub fn fetch_xor(&self, value: bool, ordering: Ordering) -> bool {
            self.0.fetch_xor(value as u32, order(ordering)) != 0
        }

        #[inline(always)]
        pub fn fetch_nand(&self, value: bool, ordering: Ordering) -> bool {
            // `nand` is not a PTX operation, so it is the one of these that is composed rather
            // than mapped: read, negate-and, and let the compare-exchange retry the race for us.
            let mut current = self.load(ordering);
            loop {
                let next = !(current & value);
                match self.compare_exchange(current, next, ordering, Ordering::Relaxed) {
                    Ok(_) => return current,
                    Err(actual) => current = actual,
                }
            }
        }
    }

    device_atomic!(AtomicU32, DeviceAtomicU32, u32);
    device_atomic!(AtomicI32, DeviceAtomicI32, i32);
    device_atomic!(AtomicU64, DeviceAtomicU64, u64);
    device_atomic!(AtomicI64, DeviceAtomicI64, i64);
}

#[cfg(feature = "cuda")]
pub use core::sync::atomic::Ordering;
#[cfg(feature = "cuda")]
pub use device::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64};
