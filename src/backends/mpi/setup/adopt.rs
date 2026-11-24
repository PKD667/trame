// The node's states and pool in its segment, built once and adopted by every other rank of the node.

extern crate std;

use std::vec::Vec;

use crate::backend::{Backend, Selected};
use crate::launch::{Plan, Refused};

use crate::backends::mpi::Transport;
use crate::backends::mpi::memory::segment;

/// Run a builder in arena mode. A builder that panics (a program's own, or the domain check of `domain::in_domain`)
/// has already printed its message; the other ranks wait at the barrier below for a builder that will not come,
/// and the panicking rank would unwind to a finalize that waits for them. So the job ends here, on every rank.
/// Run a builder in arena mode and answer the bytes it took from the domain; a panic aborts the job (see above).
fn built_metered(b: crate::launch::Builder) -> ((), u64) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::domain::arena_metered(b))) {
        Ok(((), bytes)) => ((), bytes),
        Err(_) => mpi::traits::Communicator::abort(crate::backends::mpi::setup::state::world(), 101),
    }
}

/// Put this node's states and pool in its segment (CONTRACT.md, L3, M1). The node's builder is the
/// leader on the leader's node, and the host rank elsewhere: it runs the builders of the workers on
/// its node and the pool's, in arena mode, and writes their addresses to the segment's table. After
/// a world barrier, every other rank of the node adopts them. Builders of workers on other nodes are
/// dropped unrun. Collective over the world.
///
/// The pool's builder runs first: its bytes come out of this node's memory before the workers' shares are fixed
/// (INTERFACE.md 6, Definition (Memory of a host)). A worker whose builder takes more than its share refuses the
/// launch, on every rank: the bytes go in a slot, and every rank answers the same refusal after the barrier.
pub(crate) fn build_or_adopt(plan: &mut Plan, t: &Transport) -> Result<(), Refused> {
    use std::sync::atomic::Ordering::{Acquire, Release};
    let builder = t.me() == t.lay().workers;
    let mine: Vec<usize> = (0..plan.workers.len()).collect();
    if builder {
        segment::over_slot().store(0, Release);
        let pool_bytes = match plan.pool_builder.take() {
            Some(b) => crate::domain::arena_metered(b).1,
            None => 0,
        };
        if let Some(pool) = &plan.pool {
            segment::pool_slot().store(pool.addr() as u64, Release);
        }
        let workers = mine.len() as u32;
        let share = match <Selected as Backend>::host_available() {
            Some(available) => crate::system::share(available, pool_bytes, workers),
            None => u64::MAX,
        };
        segment::share_slot().store(share, Release);
        // The builders that read it run here, before the barrier; the other ranks fix the same number after it.
        crate::system::fix(share);
        for &w in &mine {
            let b = plan.builders[w].take().expect("trame: the launch checked that every worker was set");
            let (_, bytes) = built_metered(b);
            if bytes > share {
                segment::over_slot().store(bytes, Release);
            }
            segment::state_slot(w as u32).store(plan.workers[w].addr() as u64, Release);
        }
    }
    mpi::traits::CommunicatorCollectives::barrier(&crate::backends::mpi::transport::hosts::host().comm);
    let share = segment::share_slot().load(Acquire);
    crate::system::fix(share);
    let over = segment::over_slot().load(Acquire);
    if over != 0 {
        // Every rank of this node answers the same refusal; the launch ends here, before any step.
        return Err(Refused::Share { worker: crate::topology::WorkerId::here(0), want: over, share });
    }
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
    Ok(())
}
