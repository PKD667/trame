// The launch of the transport: agreement between ranks, then the window and each rank's role. Both
// are collective over the world communicator, and every collective is reached on every rank, so a
// refusal happens on all of them or on none.

extern crate std;

use std::mem::ManuallyDrop;
use std::vec;

use mpi::collective::SystemOperation;
use mpi::topology::SimpleCommunicator;
use mpi::traits::*;

use super::layout::Topology;
use super::refusal::Refusal;
use super::shm::Window;
use super::shape::{encode, plan};
use super::types::{clock, Role};
use super::Transport;

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

/// Open the window and give this rank its role. Collective over `world`, and every rank must have
/// agreed on `topo` first (see `agree`). Refuses on every rank, or on none.
pub fn open(world: &SimpleCommunicator, topo: &Topology) -> Result<Role, Refusal> {
    let lay = plan(topo)?;
    // The node communicator is kept for as long as the window exists. It is never freed: a
    // communicator free is a point where a rank could wait for others, so this leaks it instead.
    let node = ManuallyDrop::new(world.split_shared(world.rank()));
    let one_node = i32::from(node.size() == world.size());
    let mut all = 0;
    world.all_reduce_into(&one_node, &mut all, SystemOperation::min());
    if all == 0 {
        return Err(Refusal::NotOneNode { world: world.size(), shared: node.size() });
    }
    let win = Window::allocate(&node, lay.total)?;
    if win.len() != lay.total {
        return Err(Refusal::Mpi("window size"));
    }
    let me = world.rank() as u32;
    let leader = me == topo.workers;
    let t = Transport::new(me, win, lay);
    if leader {
        // No worker has run yet. The launch barrier below keeps every worker out until this is written.
        t.win().zero();
        t.set_start(clock());
        t.install_initial(&topo.publication);
    }
    world.barrier();
    Ok(if leader { Role::Leader(t) } else { Role::Worker(t) })
}

/// `agree`, then `open`: the whole launch for callers that have no checks of their own (the tests).
pub fn launch(world: &SimpleCommunicator, topo: &Topology) -> Result<Role, Refusal> {
    agree(world, &encode(topo), topo.workers + 1)?;
    open(world, topo)
}
