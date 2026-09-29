//! Physical warp identity and the scalar worker owner.
//!
//! Lane zero owns application state, invocation and endpoints. The remaining
//! lanes leave device entry before `init`; scalar transport needs no lane vote.
//! Split and collective helpers remain independent machine utilities, not the
//! invocation lowering. The host model checks identity and splits but cannot
//! establish CUDA scheduling or atomic address-space legality.

#[cfg(feature = "cuda")]
mod cuda;
#[cfg(not(feature = "cuda"))]
mod sim;

use core::ops::Add;

#[cfg(feature = "cuda")]
pub use cuda::{all, any, ballot, lane, shuffle, sync};

#[cfg(not(feature = "cuda"))]
pub use sim::{lane, sim, sim_lane, sim_warp, sync};

#[cfg(feature = "cuda")]
pub use cuda::here_id;
#[cfg(feature = "cuda")]
pub(crate) use cuda::cohort;
#[cfg(not(feature = "cuda"))]
pub use sim::here_id;
#[cfg(not(feature = "cuda"))]
pub(crate) use sim::cohort;

/// The sole application owner of this physical warp. Check this at kernel entry,
/// before constructing application state or calling `init`.
#[inline(always)]
pub fn is_owner() -> bool {
    lane() == 0
}

/// Lanes in the current physical warp and the stride every split walk takes.
pub const LANES: u32 = 32;

/// An index type a [`Split`] may range over.
///
/// Closed on purpose. A split walk has to name the calling lane and the warp width in the
/// range's own type, and "the range's own type" is otherwise any integer-like thing an author
/// writes. Keeping the set here means the lowering needs no trait machinery at the call site and
/// a range over anything else is a compile error at the declaration rather than a surprising
/// widening.
pub trait Index: Copy + PartialOrd + Add<Output = Self> {
    /// The number `n` as this index type.
    fn of(n: u32) -> Self;
}

macro_rules! index_for {
    ($($ty:ty),+ $(,)?) => {$(
        impl Index for $ty {
            #[inline(always)]
            fn of(n: u32) -> Self {
                n as $ty
            }
        }
    )+};
}

index_for!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// One lane's share of a declared index range.
///
/// Lane `k` owns `lo + k`, `lo + k + 32`, `lo + k + 64`, … below `hi`. The parts are disjoint
/// and their union is the whole range, which is the property `tests/warp.rs` checks.
///
/// The cursor is deliberately not `Iterator`: the lowering drives it from a `while`, and an
/// iterator would put a trait method and an `Option` between the index and the loop body on a
/// path whose whole point is that it is the loop body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split<T> {
    at: T,
    hi: T,
    stride: T,
}

impl<T: Index> Split<T> {
    /// The half-open range `lo..hi`, as this lane's share of it.
    #[inline(always)]
    pub fn new(lo: T, hi: T) -> Self {
        Split {
            at: lo + T::of(lane()),
            hi,
            stride: T::of(LANES),
        }
    }

    /// The index this lane visits now. Meaningful only while [`more`](Self::more) holds.
    #[inline(always)]
    pub fn at(&self) -> T {
        self.at
    }

    /// Whether this lane still has an index to visit.
    #[inline(always)]
    pub fn more(&self) -> bool {
        self.at < self.hi
    }

    /// Advance to this lane's next index, one warp along.
    #[inline(always)]
    pub fn step(&mut self) {
        self.at = self.at + self.stride;
    }
}

/// Whether the warp's lanes can move a value between registers at all.
///
/// False on the host model, where [`any`], [`all`], [`ballot`] and [`shuffle`] are absent rather
/// than approximate. A caller that needs them is a caller that cannot be checked here, and this
/// is how it finds that out at compile time instead of at run time.
pub const COLLECTIVES: bool = cfg!(feature = "cuda");
