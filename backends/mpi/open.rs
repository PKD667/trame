// The launch of the transport: agreement between ranks, then the window and each rank's role. Both
// are collective over the world communicator, and every collective is reached on every rank, so a
// refusal happens on all of them or on none.

extern crate std;

use std::format;
use std::mem::ManuallyDrop;
use std::string::{String, ToString};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::vec;

use mpi::collective::SystemOperation;
use mpi::environment::{threading_support, Threading};
use mpi::topology::SimpleCommunicator;
use mpi::traits::*;

use super::layout::Topology;
use super::nodes::{discover, node_ranks_setting, split_groups};
use super::relay;
use super::refusal::Refusal;
use super::shape::{encode, plan};
use super::shm::Window;
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
    // A bad TRAME_MPI_NODE_RANKS is refused on every rank before any split, so no rank waits in one.
    let setting = node_ranks_setting();
    let here = i32::from(setting.is_ok());
    let mut all = 0;
    world.all_reduce_into(&here, &mut all, SystemOperation::min());
    let split = match setting {
        Ok(k) if all == 1 => k,
        Ok(_) => return Err(Refusal::NodeRanks("set to a bad value on another rank".to_string())),
        Err(text) => return Err(Refusal::NodeRanks(text)),
    };
    // The shared communicator is kept for as long as the window exists. It is never freed: a
    // communicator free is a point where a rank could wait for others, so this leaks it instead.
    let shared = world.split_shared(world.rank());
    let node = match split {
        Some(k) => {
            let group = split_groups(world, &shared, k).expect("a rank's own colour is never undefined");
            std::mem::forget(shared);
            group
        }
        None => shared,
    };
    let node = ManuallyDrop::new(node);
    let nodes = discover(world, &node);
    let threads = i32::from(nodes.count == 1 || threading_support() == Threading::Multiple);
    world.all_reduce_into(&threads, &mut all, SystemOperation::min());
    if all == 0 {
        return Err(Refusal::Threads);
    }
    let host_rank = nodes.relay_rank[nodes.me as usize];
    let name = window_name(world, host_rank);
    let win = Window::allocate(&node, lay.total, name)?;
    if win.len() != lay.total {
        return Err(Refusal::Mpi("window size"));
    }
    let me = world.rank() as u32;
    let leader = me == topo.workers;
    // The lowest rank of each node initialises that node's window: a replica of the whole layout.
    let host = host_rank == world.rank();
    let t = Transport::new(me, win, lay, Arc::new(nodes));
    // The initial publication is a POSIX object, so its creation can fail on the host. Every rank
    // refuses together, with the host's error where the host has one.
    let mut failed = 0i32;
    let mut why = String::new();
    if host {
        // No worker has run yet. The launch barrier below keeps every worker out until this is written.
        t.win().zero();
        t.set_start(clock());
        if let Err(e) = t.install_initial(&topo.publication) {
            failed = 1;
            why = e.to_string();
        }
    }
    world.all_reduce_into(&failed, &mut all, SystemOperation::max());
    if all != 0 {
        if why.is_empty() {
            why = "the node's host could not create it".into();
        }
        return Err(Refusal::Memory { need: topo.publication.len(), why });
    }
    if host && t.nodes().count > 1 {
        // Several nodes: the relay runs before the launch barrier, so every rank's first act can reach it.
        relay::start(&t, world.as_raw());
    }
    world.barrier();
    Ok(if leader { Role::Leader(t) } else { Role::Worker(t) })
}

/// The prefix of this launch's window objects (shm.rs `object`). The nonce comes from world rank 0, so
/// two launches on one machine never share a name; the node's host rank keeps two nodes of one launch
/// apart. Collective over `world`: every rank calls it.
fn window_name(world: &SimpleCommunicator, host_rank: i32) -> String {
    let mut nonce = if world.rank() == 0 { launch_nonce() } else { 0 };
    world.process_at_rank(0).broadcast_into(&mut nonce);
    format!("/trame-{nonce:016x}-{host_rank}")
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
