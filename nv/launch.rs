//! The launch description: what a launch states about itself before any participant runs.
//!
//! It exists so that this backend discovers its environment as MPI does, instead of being handed
//! one. A device has no `MPI_Comm_size` to ask, so the launcher writes the facts the participants
//! need into one symbol before the kernel starts, and `Environment::default` reads them there. That
//! makes the symbol the one place launch-supplied data enters, so it is checked there, before any
//! of it is used, and a launch that wrote nothing or wrote another layout is refused, not trusted.
//!
//! The description is launch-wide: every warp of a launch reads the same one, so it cannot hold a
//! warp's rank. A worker's rank is its warp's index, `warp::here_id`; the leader is not a warp, so
//! its launch rank is a field here.
//!
//! On the device the symbol is constant memory the launcher fills by name. The host model has no
//! kernel to launch, so its participants read a thread-local that the in-crate tests fill, which
//! keeps the discovery path the same code and adds nothing to the public surface.

use crate::contract::{BackendFault, Invalid, Launch};
use crate::nv::layout::Layout;
use crate::nv::peers::{Arena, Fabric};
use crate::nv::transport::MAX_RANKS;

/// The first word of every description: "trnv", little-endian. A zero here is a launch that wrote
/// nothing, since the symbol starts all zeros.
pub(crate) const MAGIC: u32 = u32::from_le_bytes(*b"trnv");

/// The layout this build reads. A launcher built against another layout is refused by this, or
/// by the size word, and never read as this one.
pub(crate) const VERSION: u32 = 1;

/// The `leader` word of a launch with no leader. It stays inside the description: what the backend
/// sees is `Option<Launch>`.
pub(crate) const NO_LEADER: u32 = u32::MAX;

/// The words a launcher writes. `repr(C)` because the launcher writes bytes, not a Rust value.
#[repr(C)]
#[cfg_attr(feature = "cuda", derive(Copy))]
#[derive(Clone)]
pub(crate) struct Description {
    pub(crate) magic: u32,
    pub(crate) version: u32,
    /// `size_of::<Description>()` as the launcher computed it.
    pub(crate) bytes: u32,
    /// How many workers the launch started.
    pub(crate) size: u32,
    /// The leader's launch rank, or `NO_LEADER`.
    pub(crate) leader: u32,
    /// The peer links' geometry.
    pub(crate) depth: u32,
    pub(crate) capacity: u32,
    pub(crate) arena: Arena,
    /// The leader route's region: null with zero words exactly when there is no leader.
    pub(crate) leader_region: *mut u32,
    pub(crate) leader_words: usize,
}

// SAFETY: a description is launch-wide data that participants only read; its pointers name launch
// memory whose use is governed where they are dereferenced, as `Context`'s `Send` argues, not by
// which thread holds the description. Without this the environment would be `!Send` here and
// `Send` on every other backend.
unsafe impl Send for Description {}
unsafe impl Sync for Description {}

/// What a consistent description states, in the backend's own types.
pub(crate) struct Launched {
    pub(crate) size: u32,
    pub(crate) leader: Option<Launch>,
    pub(crate) fabric: Fabric,
    pub(crate) leader_region: *mut u32,
    pub(crate) leader_words: usize,
}

impl Description {
    /// Check the description before any of it is used.
    ///
    /// This proves the description is consistent, not that its pointers are real allocations.
    pub(crate) fn check(&self) -> Result<Launched, BackendFault> {
        let inconsistent = BackendFault::Invalid(Invalid::InconsistentLaunch);
        if self.magic != MAGIC
            || self.version != VERSION
            || self.bytes as usize != size_of::<Description>()
        {
            return Err(inconsistent);
        }
        if self.size == 0 {
            return Err(inconsistent);
        }
        // The transport's tables are fixed at `MAX_RANKS`; a wider launch has no room here.
        if self.size as usize > MAX_RANKS {
            return Err(BackendFault::Storage);
        }
        let leader = (self.leader != NO_LEADER).then(|| Launch::new(self.leader));
        let region = !self.leader_region.is_null();
        if leader.is_some() != region
            || region != (self.leader_words != 0)
            || !self.leader_region.is_aligned()
        {
            return Err(inconsistent);
        }
        let layout = Layout::new(self.depth, self.capacity).map_err(|_| inconsistent)?;
        Ok(Launched {
            size: self.size,
            leader,
            fabric: Fabric::described(self.arena.clone(), layout, self.size)?,
            leader_region: self.leader_region,
            leader_words: self.leader_words,
        })
    }
}

#[cfg(feature = "cuda")]
pub(crate) use device::read;

#[cfg(feature = "cuda")]
mod device {
    use cuda_device::{ConstantMemory, ConstantMemoryValue, constant};

    use super::Description;

    // SAFETY: `Description` is `repr(C)` and `Copy`, and every field is an integer or a raw
    // pointer, so a byte-for-byte copy is a value and all zeros is a value (one whose magic is 0).
    unsafe impl ConstantMemoryValue for Description {}
    /// The launcher resolves this by its exported name, `cuda_oxide_const_246e25db_TRAME_NV_LAUNCH`,
    /// and fills it before the kernel starts.
    #[constant]
    static TRAME_NV_LAUNCH: ConstantMemory<Description> = ConstantMemory::UNINIT;

    pub(crate) fn read() -> Description {
        TRAME_NV_LAUNCH.get()
    }
}

#[cfg(not(feature = "cuda"))]
pub(crate) use model::read;
#[cfg(all(test, not(feature = "cuda")))]
pub(crate) use model::describe;

#[cfg(not(feature = "cuda"))]
mod model {
    use core::cell::RefCell;

    use super::Description;

    /// All zeros, as the device's symbol is before a launcher writes it.
    const UNWRITTEN: Description = Description {
        magic: 0,
        version: 0,
        bytes: 0,
        size: 0,
        leader: 0,
        depth: 0,
        capacity: 0,
        arena: None,
        leader_region: core::ptr::null_mut(),
        leader_words: 0,
    };

    std::thread_local! {
        /// Per thread, because the model's participants are threads and one test process runs
        /// many launches at once.
        static LAUNCH: RefCell<Description> = const { RefCell::new(UNWRITTEN) };
    }

    pub(crate) fn read() -> Description {
        LAUNCH.with(|launch| launch.borrow().clone())
    }

    /// The model's launcher: what a test writes before its participants enter.
    #[cfg(test)]
    pub(crate) fn describe(description: Description) {
        LAUNCH.with(|launch| *launch.borrow_mut() = description);
    }
}

#[cfg(all(test, not(feature = "cuda")))]
impl Description {
    /// A description with a valid header, for the model's tests to state and then spoil.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stated(
        size: u32,
        leader: Option<Launch>,
        fabric: Fabric,
        leader_region: *mut u32,
        leader_words: usize,
    ) -> Description {
        let layout = fabric.layout();
        Description {
            magic: MAGIC,
            version: VERSION,
            bytes: size_of::<Description>() as u32,
            size,
            leader: leader.map_or(NO_LEADER, Launch::get),
            depth: layout.depth(),
            capacity: layout.capacity(),
            arena: Some(fabric),
            leader_region,
            leader_words,
        }
    }
}
