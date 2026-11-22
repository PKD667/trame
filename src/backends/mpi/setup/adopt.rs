// The node's states and pool in its segment, built once and adopted by every other rank of the node.

extern crate std;

use std::vec::Vec;

use crate::launch::Plan;

use crate::backends::mpi::Transport;
use crate::backends::mpi::memory::segment;

/// Run a builder in arena mode. A builder that panics (a program's own, or the domain check of `domain::in_domain`)
/// has already printed its message; the other ranks wait at the barrier below for a builder that will not come,
/// and the panicking rank would unwind to a finalize that waits for them. So the job ends here, on every rank.
fn built(b: crate::launch::Builder) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::domain::arena(b))).is_err() {
        mpi::traits::Communicator::abort(crate::backends::mpi::setup::state::world(), 101);
    }
}

/// Put this node's states and pool in its segment (CONTRACT.md, L3, M1). The node's builder is the
/// leader on the leader's node, and the host rank elsewhere: it runs the builders of the workers on
/// its node and the pool's, in arena mode, and writes their addresses to the segment's table. After
/// a world barrier, every other rank of the node adopts them. Builders of workers on other nodes are
/// dropped unrun. Collective over the world.
pub(crate) fn build_or_adopt(plan: &mut Plan, t: &Transport) {
    use std::sync::atomic::Ordering::{Acquire, Release};
    let builder = t.me() == t.lay().workers;
    let mine: Vec<usize> = (0..plan.workers.len()).collect();
    if builder {
        for &w in &mine {
            let b = plan.builders[w].take().expect("trame: the launch checked that every worker was set");
            built(b);
            segment::state_slot(w as u32).store(plan.workers[w].addr() as u64, Release);
        }
        if let Some(b) = plan.pool_builder.take() {
            built(b);
            let pool = plan.pool.as_ref().expect("trame: a pool builder belongs to a declared pool");
            segment::pool_slot().store(pool.addr() as u64, Release);
        }
    }
    mpi::traits::CommunicatorCollectives::barrier(&crate::backends::mpi::transport::hosts::host().comm);
    if !builder {
        for &w in &mine {
            // SAFETY: the node's builder filled a Core of the same type at this address, in the
            // segment this rank maps at the same address (segment.rs; CONTRACT.md, M1).
            unsafe { plan.workers[w].adopt(segment::state_slot(w as u32).load(Acquire) as usize) };
        }
        if let Some(pool) = &plan.pool {
            // SAFETY: as above, for the pool.
            unsafe { pool.adopt(segment::pool_slot().load(Acquire) as usize) };
        }
    }
}
