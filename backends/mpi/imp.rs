// The Backend trait on the MPI transport (spec.md 6). Rank i < W is worker i; rank W is the leader.
// Every rank runs the host's code up to `launch`. A worker rank then runs its function here and never
// returns to the host: it records its outcome in the window and ends the process. The leader rank
// returns the Run, and the host talks to the workers through it.

extern crate std;

use core::any::TypeId;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process;
use std::sync::Arc;
use std::vec::Vec;

use crate::backend::Backend;
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::time::ClockId;
use crate::topology::{Context, WorkerId};

use super::layout::{OUT_ABORT, OUT_DEADLOCK, OUT_RETURNED};
use super::leader::{self, Leader};
use super::open::{agree, open};
use super::plan::{encode_plan, entries, inbox, topology};
use super::state::{finalize, local, refused, world};
use super::types::Role;
use super::{Selected, Transport, worker};
use crate::run::panic_message;

/// Run this worker's function on the main thread. A return records Returned; stuck at a barrier, Deadlock.
/// A panic records the abort with its message (results.rs `abort`), and an abort that reached this worker
/// records only its own result. The result is recorded before anything waits on a peer: then MPI_Finalize
/// (state.rs), which waits for every rank, and then the process ends.
fn run_worker(f: &(dyn Fn(&Context) + Send), me: u32, workers: u32) -> ! {
    let ids: Vec<WorkerId> = (0..workers).map(WorkerId).collect();
    let ctx = Context::new(WorkerId(me), ids);
    let result = catch_unwind(AssertUnwindSafe(|| f(&ctx)));
    let t: &Transport = local().endpoint.as_ref().expect("trame: a worker has an endpoint");
    match result {
        Ok(()) => {
            t.finish(OUT_RETURNED);
        }
        // Stuck at a barrier: the result is Deadlock, and the run goes on for the others (spec.md 5).
        Err(p) if p.is::<worker::Stuck>() => {
            t.finish(OUT_DEADLOCK);
        }
        // The run aborted before this worker panicked: its first record stands.
        Err(p) if p.is::<worker::Aborted>() => {
            t.finish(OUT_ABORT);
        }
        Err(p) => t.abort(panic_message(&*p).as_bytes()),
    }
    finalize();
    process::exit(0)
}

impl Backend for Selected {
    type Run = Leader;

    fn agree(plan: &Plan) -> Result<(), Refused> {
        let ranks = plan.workers.len() as u32 + 1;
        agree(world(), &encode_plan(plan), ranks).map_err(refused)
    }

    fn launch(plan: Plan) -> Result<Leader, Refused> {
        let topo = topology(&plan)?;
        let role = open(world(), &topo).map_err(refused)?;
        let table = entries(&plan);
        let l = local();
        l.entries = table;
        match role {
            Role::Leader(t) => {
                l.inbox = inbox(&l.entries, t.me());
                Ok(Leader::new(t))
            }
            Role::Worker(t) => {
                let me = t.me();
                l.inbox = inbox(&l.entries, me);
                let f = plan.workers[me as usize].as_ref();
                l.endpoint = Some(t);
                run_worker(f, me, topo.workers)
            }
        }
    }

    fn worker_send(to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        worker::send(to, ty, name, bytes)
    }

    fn worker_recv(ty: TypeId, out: &mut [u8]) -> bool {
        worker::recv(ty, out)
    }

    fn worker_offer(ty: TypeId) -> bool {
        worker::offer(ty)
    }

    fn worker_route(to: WorkerId, ty: TypeId) -> Option<usize> {
        worker::route_index(to, ty)
    }

    fn worker_room(route: usize) -> bool {
        worker::room(route)
    }

    fn worker_epoch() -> u64 {
        worker::epoch()
    }

    fn worker_idle(until: Option<u64>, epoch: u64) {
        worker::idle(until, epoch)
    }

    fn worker_now() -> (ClockId, u64) {
        worker::now()
    }

    fn worker_barrier() -> Arc<[u8]> {
        worker::barrier()
    }

    fn leader_send(run: &Leader, to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        leader::send(run, to, ty, name, bytes)
    }

    fn leader_recv(run: &Leader, ty: TypeId, out: &mut [u8]) -> bool {
        leader::recv(run, ty, out)
    }

    fn leader_publish(run: &Leader, bytes: &[u8]) {
        leader::publish(run, bytes)
    }

    fn leader_wait(run: &Leader, seen: u64) -> u64 {
        leader::wait(run, seen)
    }

    fn leader_join(run: Leader) -> Vec<Outcome> {
        leader::join(run)
    }
}
