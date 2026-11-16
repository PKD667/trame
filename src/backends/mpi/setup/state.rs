// The state of this process in the launch, and the MPI world it uses. The backend operations take no
// sender and no run handle for workers, so a worker's endpoint lives here.
//
// Only the rank's main thread touches this. The worker function runs there (imp.rs), and so do the
// leader's calls. A host that spawns threads and calls the backend from them is outside this
// backend's contract: the global is not synchronised, and nothing here is `Sync`.

extern crate std;

use std::cell::UnsafeCell;
use std::vec::Vec;

use mpi::topology::SimpleCommunicator;

use crate::launch::Refused;

use crate::backends::mpi::setup::plan::Entry;
use crate::backends::mpi::setup::refusal::Refusal;
use crate::backends::mpi::{RouteId, Transport};

pub(crate) struct Local {
    pub world: Option<SimpleCommunicator>,
    /// Every route of the plan, for lookups by rank and type.
    pub entries: Vec<Entry>,
    /// The routes this rank receives, grouped by message type (plan::inbox).
    pub inbox: Vec<(core::any::TypeId, Vec<RouteId>)>,
    /// The worker endpoint, once this rank has opened the window as a worker.
    pub endpoint: Option<Transport>,
    /// This process's host (hosts.rs).
    pub host: Option<crate::backends::mpi::transport::hosts::Host>,
}

struct Slot(UnsafeCell<Option<Local>>);

// SAFETY: sound only under the contract in the module comment: one thread per rank touches the slot.
unsafe impl Sync for Slot {}

static LOCAL: Slot = Slot(UnsafeCell::new(None));

/// This process's state. The caller must not keep the reference across another call to `local`.
pub(crate) fn local() -> &'static mut Local {
    // SAFETY: one thread per rank (see the module comment), and no reference is held across calls.
    let slot = unsafe { &mut *LOCAL.0.get() };
    slot.get_or_insert_with(|| Local {
        world: None,
        entries: Vec::new(),
        inbox: Vec::new(),
        endpoint: None,
        host: None,
    })
}

/// The MPI world of this process. MPI is initialised once; the universe is forgotten, not dropped,
/// so that its drop does not finalize at a moment of our choosing. `finalize` does that instead.
pub(crate) fn world() -> &'static SimpleCommunicator {
    let l = local();
    if l.world.is_none() {
        if let Some((universe, _)) = mpi::initialize_with_threading(mpi::Threading::Multiple) {
            std::mem::forget(universe);
            // A process that exits without `join` (a refusal, a dropped run) still finalizes.
            extern "C" fn at_exit() {
                finalize();
            }
            // SAFETY: `at_exit` is a plain function and takes no arguments.
            unsafe { libc::atexit(at_exit) };
        }
        l.world = Some(SimpleCommunicator::world());
    }
    l.world.as_ref().expect("the world was just set")
}

/// Call MPI_Finalize once, if MPI is up and not yet finalized. Finalize waits for every rank, so it
/// is called only after this rank's result is final (see the README, Finalization).
pub fn finalize() {
    let (mut up, mut done): (i32, i32) = (0, 0);
    // SAFETY: each query takes one out-pointer. The check is on MPI itself, not on `world()`: a test
    // harness that initialises MPI on its own never sets `world`.
    unsafe {
        mpi::ffi::MPI_Initialized(&mut up);
        mpi::ffi::MPI_Finalized(&mut done);
    }
    if up == 0 || done != 0 {
        return;
    }
    // Every rank that made the hosts takes part in the lanes' end (cross.rs `quiesce`), with or
    // without lanes of its own: the leader's endpoint has none.
    let l = local();
    if l.host.is_some() {
        match l.endpoint.as_mut() {
            Some(t) => t.quiesce(),
            None => crate::backends::mpi::transport::cross::quiesce(&mut []),
        }
    }
    // SAFETY: MPI is up and not finalized; every lane's message is complete.
    unsafe { mpi::ffi::MPI_Finalize() };
}

/// Map a transport refusal to the surface's, with its message whole (refusal.rs).
pub(crate) fn refused(r: Refusal) -> Refused {
    Refused::Backend { reason: alloc::format!("{r}") }
}
