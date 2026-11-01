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

use super::plan::Entry;
use super::refusal::Refusal;
use super::relay::Relay;
use super::{RouteId, Transport};

pub(super) struct Local {
    pub world: Option<SimpleCommunicator>,
    /// Every route of the plan, for lookups by rank and type.
    pub entries: Vec<Entry>,
    /// The routes this rank receives, grouped by message type (plan::inbox).
    pub inbox: Vec<(core::any::TypeId, Vec<RouteId>)>,
    /// The worker endpoint, once this rank has opened the window as a worker.
    pub endpoint: Option<Transport>,
    /// This node's relay threads, when this rank hosts them (relay.rs). `finalize` joins them.
    pub relay: Option<Relay>,
}

struct Slot(UnsafeCell<Option<Local>>);

// Sound only under the contract in the module comment: one thread per rank touches the slot.
unsafe impl Sync for Slot {}

static LOCAL: Slot = Slot(UnsafeCell::new(None));

/// This process's state. The caller must not keep the reference across another call to `local`.
pub(super) fn local() -> &'static mut Local {
    // SAFETY: one thread per rank (see the module comment), and no reference is held across calls.
    let slot = unsafe { &mut *LOCAL.0.get() };
    slot.get_or_insert_with(|| Local {
        world: None,
        entries: Vec::new(),
        inbox: Vec::new(),
        endpoint: None,
        relay: None,
    })
}

/// The MPI world of this process. MPI is initialised once; the universe is forgotten, not dropped,
/// so that its drop does not finalize at a moment of our choosing. `finalize` does that instead.
pub(super) fn world() -> &'static SimpleCommunicator {
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
    // The relay threads call MPI, so they stop before MPI does (NODES.md, End).
    if let Some(relay) = local().relay.take() {
        relay.join();
    }
    let (mut up, mut done): (i32, i32) = (0, 0);
    // SAFETY: each query takes one out-pointer; MPI_Finalize takes none. The check is on MPI itself,
    // not on `world()`: a test harness that initialises MPI on its own never sets `world`.
    unsafe {
        mpi::ffi::MPI_Initialized(&mut up);
        mpi::ffi::MPI_Finalized(&mut done);
        if up != 0 && done == 0 {
            mpi::ffi::MPI_Finalize();
        }
    }
}

/// Map a transport refusal to the surface's. The surface's reason is a static string, so the detail
/// that varies (a count, a size) is not in it; the transport's own message has it.
pub(super) fn refused(r: Refusal) -> Refused {
    let reason = match r {
        Refusal::Topology(_) => "the MPI transport's layout refused the topology",
        Refusal::Inconsistent => "the ranks were given different plans",
        Refusal::Ranks { .. } => "the world does not have one rank per worker plus the leader",
        Refusal::Memory { .. } => "the MPI backend could not allocate the shared memory the launch needs",
        Refusal::NodeRanks(_) => "TRAME_MPI_NODE_RANKS is not a number of ranks per node",
        Refusal::Threads => "a launch over several nodes needs MPI_THREAD_MULTIPLE",
        Refusal::NotUnified => "the MPI window is not in the unified memory model",
        Refusal::Mpi(_) => "an MPI call failed",
        Refusal::SmallSetting(_) => "TRAME_MPI_SMALL is not p2p or rma",
        Refusal::Rma(_) => "the RMA lanes could not be made (MPI_THREAD_MULTIPLE and a unified window are needed)",
    };
    Refused::Backend { reason }
}
