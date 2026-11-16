// The launch of the transport: agreement between ranks, then the window and each rank's role. Both
// are collective over the world communicator, and every collective is reached on every rank, so a
// refusal happens on all of them or on none.

extern crate std;

use std::format;
use std::mem::ManuallyDrop;
use std::string::{String, ToString};
use std::time::{SystemTime, UNIX_EPOCH};
use std::vec;

use mpi::collective::SystemOperation;
use mpi::topology::SimpleCommunicator;
use mpi::traits::*;

use crate::backends::mpi::Transport;
use crate::backends::mpi::memory::layout::Topology;
use crate::backends::mpi::memory::segment;
use crate::backends::mpi::memory::shm::Window;
use crate::backends::mpi::setup::refusal::Refusal;
use crate::backends::mpi::setup::shape::{encode, plan};
use crate::backends::mpi::transport::nodes::node_ranks_setting;
use crate::backends::mpi::types::{Role, clock};

/// Every rank of the launch checks that it was given the same bytes. Collective over `world`: every
/// rank calls it, and every rank gets the same answer. The bytes of the last rank (the leader, when
/// the world has `ranks` ranks) are the reference. Refuses when the world has another rank count.
pub fn agree(world: &SimpleCommunicator, bytes: &[u8], ranks: u32) -> Result<(), Refusal> {
    let last = world.size() - 1;
    let root = world.process_at_rank(last);
    let mut n = bytes.len() as u64;
    root.broadcast_into(&mut n);
    let mut theirs = vec![0u8; n as usize];
    if world.rank() == last {
        theirs.copy_from_slice(bytes);
    }
    root.broadcast_into(&mut theirs[..]);
    let fits = world.size() == ranks as i32;
    let here = i32::from(theirs == bytes && fits);
    let mut all = 0;
    world.all_reduce_into(&here, &mut all, SystemOperation::min());
    if all == 1 {
        Ok(())
    } else if !fits {
        Err(Refusal::Ranks { workers: ranks - 1, ranks: world.size() as u32 })
    } else {
        Err(Refusal::Inconsistent)
    }
}

/// Open the window and give this rank its role. Collective over `world`, the host's communicator
/// (hosts.rs): its ranks share memory. Every rank must have agreed on `topo` first (see `agree`).
/// Refuses on every rank, or on none.
pub fn open(world: &SimpleCommunicator, topo: &Topology) -> Result<Role, Refusal> {
    let lay = plan(topo)?;
    // A bad TRAME_MPI_NODE_RANKS is refused on every rank, so no rank waits in a collective alone.
    let setting = node_ranks_setting();
    let here = i32::from(setting.is_ok());
    let mut all = 0;
    world.all_reduce_into(&here, &mut all, SystemOperation::min());
    match setting {
        Ok(_) if all == 1 => {}
        Ok(_) => return Err(Refusal::NodeRanks("set to a bad value on another rank".to_string())),
        Err(text) => return Err(Refusal::NodeRanks(text)),
    }
    // The shared communicator is kept for as long as the window exists. It is never freed: a
    // communicator free is a point where a rank could wait for others, so this leaks it instead.
    let node = ManuallyDrop::new(world.split_shared(world.rank()));
    let whole = i32::from(node.size() == world.size());
    world.all_reduce_into(&whole, &mut all, SystemOperation::min());
    if all == 0 {
        return Err(Refusal::Mpi("a host's ranks do not all share memory"));
    }
    let name = window_name(world);
    let seg = format!("{name}-seg");
    let win = Window::allocate(&node, lay.total, name)?;
    if win.len() != lay.total {
        return Err(Refusal::Mpi("window size"));
    }
    let me = world.rank() as u32;
    let leader = me == topo.workers;
    // Rank 0 initialises the window and creates the segment (segment.rs); then every other rank maps
    // it. Refused on every rank if one rank could not map it; the name is removed once every rank has
    // tried, mapped or not.
    let first = world.rank() == 0;
    let created = if first { segment::map(&seg, true, topo.workers) } else { Ok(()) };
    node.barrier();
    let mapped = if first { created } else { segment::map(&seg, false, topo.workers) };
    let here = i32::from(mapped.is_ok());
    world.all_reduce_into(&here, &mut all, SystemOperation::min());
    node.barrier();
    if first {
        segment::unlink(&seg);
    }
    if all == 0 {
        let why = mapped.err().unwrap_or_else(|| "another rank could not map the segment".to_string());
        return Err(Refusal::Memory { need: segment::LEN, why });
    }
    let mut t = Transport::new(me, win, lay);
    t.open_lanes();
    if first {
        // No worker has run yet. The launch barrier below keeps every worker out until this is written.
        t.win().zero();
        t.set_start(clock());
    }
    world.barrier();
    Ok(if leader { Role::Leader(t) } else { Role::Worker(t) })
}

/// The prefix of this launch's window objects (shm.rs `object`). The nonce comes from rank 0, so two
/// launches on one machine never share a name. Collective over `world`: every rank calls it.
fn window_name(world: &SimpleCommunicator) -> String {
    let mut nonce = if world.rank() == 0 { launch_nonce() } else { 0 };
    world.process_at_rank(0).broadcast_into(&mut nonce);
    format!("/trame-{nonce:016x}")
}
fn launch_nonce() -> u64 {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    nanos ^ (u64::from(std::process::id()) << 32)
}

/// `agree`, then `open`: the whole launch for callers that have no checks of their own (the tests).
pub fn launch(world: &SimpleCommunicator, topo: &Topology) -> Result<Role, Refusal> {
    agree(world, &encode(topo), topo.workers + 1)?;
    open(world, topo)
}
