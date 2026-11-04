//! `Plain`: the kind of a value a state or the pool may hold (CONTRACT.md, M2 and M3). A state lives
//! in its host's memory domain and is read in place by every process and device of the host, so a
//! value in it must mean the same in each: its bytes, and the blocks it owns in the domain, are all
//! of it. Numbers, `bool`, `char`, arrays, tuples, `Option`, `Vec`, `Box`, `String` and the `alloc`
//! collections are plain when what they hold is; a struct or enum is plain by
//! `#[derive(trame::Plain)]`, which checks every field. A reference, a raw pointer, a `fn` pointer,
//! a `dyn` value, `Rc`, `Arc`, a cell, an atomic and a lock are not: each names memory or code of
//! one process, or changes outside a step.
//!
//! The check is on types. A `usize` that holds an address is plain, and means nothing in another
//! process.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::num::{Saturating, Wrapping};

/// A type whose values a trame state or pool may hold (see the module comment).
///
/// # Safety
/// An implementation asserts CONTRACT.md M2 and M3 for the type: it holds no reference, pointer,
/// `fn`, `dyn`, shared ownership, cell, atomic or lock, at any depth, and owns memory only through
/// `alloc` types. `#[derive(trame::Plain)]` writes the implementation after checking every field.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be held by a trame state or pool",
    label = "not `trame::Plain`",
    note = "a state holds plain data: numbers, bool, char, arrays, tuples, Option, Vec, Box, String, the alloc collections, and types with #[derive(trame::Plain)]",
    note = "references, raw pointers, fn pointers, dyn values, Rc, Arc, cells, atomics and locks mean nothing in another process of the host (CONTRACT.md M2, M3)"
)]
pub unsafe trait Plain: Send + Sync + 'static {}

macro_rules! plain {
    ($($t:ty),* $(,)?) => { $(
        // SAFETY: a value of this type is its bytes.
        unsafe impl Plain for $t {}
    )* };
}

plain!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, bool, char, (), String);
plain!(
    core::num::NonZeroU8,
    core::num::NonZeroU16,
    core::num::NonZeroU32,
    core::num::NonZeroU64,
    core::num::NonZeroU128,
    core::num::NonZeroUsize,
    core::num::NonZeroI8,
    core::num::NonZeroI16,
    core::num::NonZeroI32,
    core::num::NonZeroI64,
    core::num::NonZeroI128,
    core::num::NonZeroIsize,
);
plain!(core::time::Duration, crate::time::Reading, crate::topology::WorkerId, crate::topology::HostId);

macro_rules! plain_of {
    ($($c:ident),* $(,)?) => { $(
        // SAFETY: owns its elements in the domain, and each is plain.
        unsafe impl<T: Plain> Plain for $c<T> {}
    )* };
}

plain_of!(Option, Vec, VecDeque, BTreeSet, BinaryHeap, Wrapping, Saturating);

// SAFETY: owns its value in the domain, and the value is plain.
unsafe impl<T: ?Sized + Plain> Plain for Box<T> {}
// SAFETY: its elements are inline, and each is plain (behind a `Box`, as `Box<[T]>`).
unsafe impl<T: Plain> Plain for [T] {}
// SAFETY: its bytes are its value (behind a `Box`, as `Box<str>`).
unsafe impl Plain for str {}
// SAFETY: holds no value.
unsafe impl<T: ?Sized + Send + Sync + 'static> Plain for PhantomData<T> {}
// SAFETY: its elements are inline, and each is plain.
unsafe impl<T: Plain, const N: usize> Plain for [T; N] {}
// SAFETY: owns its keys and values in the domain, and each is plain.
unsafe impl<K: Plain, V: Plain> Plain for BTreeMap<K, V> {}
// SAFETY: either value is plain.
unsafe impl<T: Plain, E: Plain> Plain for Result<T, E> {}

macro_rules! plain_tuple {
    ($($t:ident)+) => {
        // SAFETY: its elements are inline, and each is plain.
        unsafe impl<$($t: Plain),+> Plain for ($($t,)+) {}
    };
}

plain_tuple!(A);
plain_tuple!(A B);
plain_tuple!(A B C);
plain_tuple!(A B C D);
plain_tuple!(A B C D E);
plain_tuple!(A B C D E F);
plain_tuple!(A B C D E F G);
plain_tuple!(A B C D E F G H);
