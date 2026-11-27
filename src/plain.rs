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
use core::cmp::Reverse;
use core::marker::PhantomData;
use core::num::{Saturating, Wrapping};

/// A type whose values a trame state or pool may hold (see the module comment).
///
/// # Safety
/// An implementation asserts CONTRACT.md M2 and M3 for the type: it holds no reference, pointer,
/// `fn`, `dyn`, shared ownership, cell, atomic or lock, at any depth, and owns memory only through
/// `alloc` types. `#[derive(trame::Plain)]` writes the implementation after checking every field.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be held by a trame state, pool or error",
    label = "not `trame::Plain`",
    note = "a state holds plain data: numbers, bool, char, arrays, tuples, Option, Vec, Box, String, the alloc collections, and types with #[derive(trame::Plain)]",
    note = "references, raw pointers, fn pointers, dyn values, Rc, Arc, cells, atomics and locks mean nothing in another process of the host (CONTRACT.md M2, M3)"
)]
pub unsafe trait Plain: Send + Sync + 'static {
    /// Calls `at` with the address of every block this value owns, and answers whether `at` accepted each. A
    /// backend whose states live in a domain shared by several processes uses it, at the end of a builder, to
    /// find a block the builder did not allocate in the domain: a `Vec` built before the builder and moved in
    /// keeps the heap of the process that built it, and the other processes then read an address that means
    /// nothing to them (INTERFACE.md 3). The default owns none. `Vec`, `Box`, `String`, `VecDeque`,
    /// `BinaryHeap` and the types that hold them answer; a `BTreeMap` or `BTreeSet` answers for its elements and
    /// not its nodes, which `alloc` does not show. A hand-written `unsafe impl Plain` that owns a block answers
    /// for it.
    fn blocks(&self, _at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        true
    }
}

macro_rules! plain {
    ($($t:ty),* $(,)?) => { $(
        // SAFETY: a value of this type is its bytes.
        unsafe impl Plain for $t {}
    )* };
}

plain!(
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64,
    bool,
    char,
    (),
    core::convert::Infallible
);
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
plain!(
    core::time::Duration,
    crate::time::Reading,
    crate::time::ClockId,
    crate::vocab::WorkerId,
    crate::vocab::HostId,
    crate::topology::Leader,
    crate::vocab::End
);

/// What the `blocks` a macro writes calls on each field (crates/macros/state.rs). A field whose type is not `Plain` is
/// refused by the check beside it, with the span of its type; calling `Plain::blocks` on it would refuse it a second
/// time, at the attribute. So the call resolves by autoref: `Plain` types answer through `Yes`, the others through
/// `No`, which answers true, and the refusal stays one.
#[doc(hidden)]
pub mod probe {
    use super::Plain;
    pub struct Field<'a, T: ?Sized>(pub &'a T);
    pub trait Yes {
        fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool;
    }
    impl<T: Plain + ?Sized> Yes for Field<'_, T> {
        fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
            self.0.blocks(at)
        }
    }
    pub trait No {
        fn blocks(&self, _at: &mut dyn FnMut(*const u8) -> bool) -> bool {
            true
        }
    }
    impl<T: ?Sized> No for &Field<'_, T> {}
}

/// A buffer of `n` elements of `T` at `p`, with a block when it has bytes.
fn block<T>(p: *const T, cap: usize, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
    cap == 0 || core::mem::size_of::<T>() == 0 || at(p.cast())
}

/// Every element of a slice.
fn each<T: Plain>(xs: &[T], at: &mut dyn FnMut(*const u8) -> bool) -> bool {
    xs.iter().all(|x| x.blocks(at))
}

macro_rules! plain_of {
    ($($c:ident),* $(,)?) => { $(
        // SAFETY: owns its element inline, and it is plain.
        unsafe impl<T: Plain> Plain for $c<T> {}
    )* };
}

plain_of!(Wrapping, Saturating, Reverse);

// SAFETY: its bytes are its value, in a block it owns.
unsafe impl Plain for String {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        block(self.as_ptr(), self.capacity(), at)
    }
}
// SAFETY: owns its value inline, and the value is plain.
unsafe impl<T: Plain> Plain for Option<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        self.as_ref().is_none_or(|x| x.blocks(at))
    }
}
// SAFETY: owns its elements in the domain, and each is plain.
unsafe impl<T: Plain> Plain for Vec<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        block(self.as_ptr(), self.capacity(), at) && each(self, at)
    }
}
// SAFETY: owns its elements in the domain, and each is plain.
unsafe impl<T: Plain> Plain for VecDeque<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        let (a, b) = self.as_slices();
        // The ring's buffer starts at the first slice when it is not empty, and its start is not exposed
        // when the deque is empty (then it has no element to read, but may own a block: unchecked).
        (self.is_empty() || block(a.as_ptr(), self.capacity(), at)) && each(a, at) && each(b, at)
    }
}
// SAFETY: owns its elements in the domain, and each is plain.
unsafe impl<T: Plain> Plain for BinaryHeap<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        let xs = self.as_slice();
        block(xs.as_ptr(), self.capacity(), at) && each(xs, at)
    }
}
// SAFETY: owns its elements in nodes of the domain, and each is plain. The nodes are not shown.
unsafe impl<T: Plain> Plain for BTreeSet<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        self.iter().all(|x| x.blocks(at))
    }
}

// SAFETY: owns its value in the domain, and the value is plain.
unsafe impl<T: ?Sized + Plain> Plain for Box<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        let r: &T = self;
        (core::mem::size_of_val(r) == 0 || at((r as *const T).cast())) && r.blocks(at)
    }
}
// SAFETY: a `Bounded` is a `Vec` and a length bound: plain data, owned, with no handle to this process.
unsafe impl<T: Plain> Plain for crate::bounded::Bounded<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        self.vec().blocks(at)
    }
}
// SAFETY: as `Bounded`, over a heap.
unsafe impl<T: Plain + Ord> Plain for crate::bounded::BoundedHeap<T> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        self.vec().blocks(at)
    }
}
// SAFETY: its elements are inline, and each is plain (behind a `Box`, as `Box<[T]>`).
unsafe impl<T: Plain> Plain for [T] {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        each(self, at)
    }
}
// SAFETY: its bytes are its value (behind a `Box`, as `Box<str>`).
unsafe impl Plain for str {}
// SAFETY: holds no value.
unsafe impl<T: ?Sized + Send + Sync + 'static> Plain for PhantomData<T> {}
// SAFETY: its elements are inline, and each is plain.
unsafe impl<T: Plain, const N: usize> Plain for [T; N] {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        each(self, at)
    }
}
// SAFETY: owns its keys and values in the domain, and each is plain. The nodes are not shown.
unsafe impl<K: Plain, V: Plain> Plain for BTreeMap<K, V> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        self.iter().all(|(k, v)| k.blocks(at) && v.blocks(at))
    }
}
// SAFETY: either value is plain.
unsafe impl<T: Plain, E: Plain> Plain for Result<T, E> {
    fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
        match self {
            Ok(x) => x.blocks(at),
            Err(e) => e.blocks(at),
        }
    }
}

macro_rules! plain_tuple {
    ($($t:ident)+) => {
        // SAFETY: its elements are inline, and each is plain.
        unsafe impl<$($t: Plain),+> Plain for ($($t,)+) {
            #[allow(non_snake_case)]
            fn blocks(&self, at: &mut dyn FnMut(*const u8) -> bool) -> bool {
                let ($($t,)+) = self;
                true $(&& $t.blocks(at))+
            }
        }
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
