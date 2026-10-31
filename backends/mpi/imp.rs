// The Backend trait on the MPI transport (spec.md 6). Rank i < W is worker i; rank W is the leader.
// Every rank runs the host's code up to `launch`, so every rank builds every state; a worker rank keeps
// its own and runs it here with several agents, and never returns to the host: it records its outcome
// in the window and ends the process. The leader rank returns the Run, and the host talks to the
// workers through it. The leader does not hold the workers' states, so Run::edit, pools and
// Worker::take are not built here yet (they need the state shipped between ranks).

extern crate std;

use core::any::TypeId;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process;
use std::sync::Arc;
use std::vec::Vec;

use crate::backend::{Backend, PoolRef};
use crate::sched::{Answer, Event, Sched};
use crate::worker::{Erased, Slot, agent};
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::time::ClockId;
use crate::topology::WorkerId;

use super::layout::{OUT_ABORT, OUT_FAILED, OUT_RETURNED, OUT_STOPPED};
use super::leader::{self, Leader};
use super::open::{agree, open};
use super::plan::{encode_plan, entries, inbox, topology};
use super::state::{finalize, local, refused, world};
use super::types::Role;
use super::{Selected, Transport, worker};
use crate::run::panic_message;

/// Run this worker: `agents` agents of the reference scheduler (worker.rs), the main thread one of
/// them. A panic in a step records the abort at once (results.rs `abort`), which wakes every sleeping
/// agent of the run so it unwinds; an abort that reached this worker first records only its own result.
/// Otherwise the worker's answer is its result. The result is recorded before anything waits on a
/// peer: then MPI_Finalize (state.rs), which waits for every rank, and then the process ends.
fn run_worker(obj: Arc<dyn Erased>, agents: usize) -> ! {
    worker::fast_init();
    let slot = Slot::new(obj, false);
    let start = std::time::Instant::now();
    let clock = || u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let no_pool = || None;
    let one = || {
        let r = catch_unwind(AssertUnwindSafe(|| agent(&slot, &clock, &no_pool)));
        if let Err(p) = &r
            && !p.is::<worker::Aborted>()
        {
            // The first panic of the run; a later one is ignored by `abort` (results.rs).
            with_endpoint(|t| t.abort(panic_message(&**p).as_bytes()));
        }
        r.ok()
    };
    let answers: Vec<Option<Answer>> = std::thread::scope(|s| {
        let others: Vec<_> = (1..agents).map(|_| s.spawn(one)).collect();
        let mut out = std::vec![one()];
        out.extend(others.into_iter().map(|h| h.join().unwrap_or(None)));
        out
    });
    let word = match answers.iter().flatten().next() {
        _ if answers.iter().any(Option::is_none) => OUT_ABORT,
        Some(Answer::Returned) => OUT_RETURNED,
        Some(Answer::Stopped) => OUT_STOPPED,
        Some(Answer::Failed(_)) => OUT_FAILED,
        None => OUT_ABORT,
    };
    with_endpoint(|t| t.finish(word));
    finalize();
    process::exit(0)
}

/// Run `f` on this rank's endpoint, under the rank's transport lock.
fn with_endpoint<R>(f: impl FnOnce(&Transport) -> R) -> R {
    worker::with_endpoint(f)
}

/// Agents per worker: this node's cores shared among its ranks, at most the worker's steps.
fn agents_for(obj: &dyn Erased, t: &Transport, cap: Option<usize>) -> usize {
    let nodes = t.nodes();
    let here = nodes.node_of.iter().filter(|&&n| n == nodes.me).count().max(1);
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let fair = (cores / here).max(1);
    cap.unwrap_or(fair).min(fair).min(obj.steps().len()).max(1)
}

impl Backend for Selected {
    type Run = Leader;

    const STATES_AT_LEADER: bool = false;

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
                let obj = plan.workers[me as usize].clone();
                let agents = agents_for(&*obj, &t, plan.agents);
                l.endpoint = Some(t);
                run_worker(obj, agents)
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

    fn worker_abandon() -> ! {
        std::panic::resume_unwind(std::boxed::Box::new(worker::Aborted))
    }

    fn worker_me() -> WorkerId {
        WorkerId(worker::me())
    }

    fn worker_stop_requested() -> bool {
        worker::stop_requested()
    }

    fn leader_send(run: &Leader, to: WorkerId, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        leader::send(run, to, ty, name, bytes)
    }

    fn leader_recv(run: &Leader, ty: TypeId, out: &mut [u8]) -> bool {
        leader::recv(run, ty, out)
    }

    fn leader_pool(_run: &Leader, _pool: PoolRef) {
        panic!("trame: pools are not carried to worker ranks on mpi yet (docs/worker-model.md, MPI)")
    }

    fn leader_edit(_run: &Leader, _w: usize, _f: &mut dyn FnMut()) -> bool {
        panic!("trame: Run::edit needs the state shipped between ranks, not built on mpi yet (docs/worker-model.md, MPI)")
    }

    fn leader_stop(run: &Leader) {
        leader::stop(run)
    }

    fn leader_join_traced(run: Leader, workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>) {
        (leader::join(run), (0..workers).map(|_| None).collect())
    }

    fn leader_wait(run: &Leader, seen: u64) -> u64 {
        leader::wait(run, seen)
    }

    fn leader_join(run: Leader) -> Vec<Outcome> {
        leader::join(run)
    }
}
