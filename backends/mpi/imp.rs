// The Backend trait on the MPI transport (spec.md 6). Rank i < W is worker i; rank W is the leader.
// Every rank runs the host's code up to `launch`. Each node has a segment (segment.rs), its workers'
// domain: the leader builds the states of the workers on its node, and the pool, in its node's
// segment; the host rank of another node builds that node's. Every other rank adopts the addresses
// from the segment's table. A worker rank runs its state in place with several agents, and never
// returns to the host: it records its outcome in the window and ends the process. The leader rank
// returns the Run; `Run::edit`, `Worker::take` and `Worker::error` act on the segment, for the workers
// on the leader's node.

extern crate std;

use core::any::TypeId;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process;
use std::sync::Arc;
use std::vec::Vec;

use crate::backend::{Backend, PoolRef, Target};
use crate::sched::{Answer, Event, Sched};
use crate::worker::{Erased, Slot, agent};
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::time::ClockId;
use crate::topology::{End, WorkerId};

use super::layout::{OUT_ABORT, OUT_FAILED, OUT_RETURNED, OUT_STOPPED};
use super::leader::{self, Leader};
use super::open::{agree, open};
use super::plan::{encode_plan, entries, inbox, topology};
use super::state::{finalize, local, refused};
use super::types::Role;
use super::{Selected, Transport, segment, worker};
use crate::run::panic_message;

/// Run this worker: `agents` agents of the reference scheduler (worker.rs), the main thread one of
/// them. A panic in a step records the abort at once (results.rs `abort`), which wakes every sleeping
/// agent of the run so it unwinds; an abort that reached this worker first records only its own result.
/// Otherwise the worker's answer is its result. The result is recorded before anything waits on a
/// peer: then MPI_Finalize (state.rs), which waits for every rank, and then the process ends.
fn run_worker(obj: Arc<dyn Erased>, agents: usize, pool: Option<PoolRef>, trace: bool) -> ! {
    worker::fast_init();
    let slot = Slot::new(obj, trace);
    let pool = pool.as_deref().map(|p| p.any());
    let start = std::time::Instant::now();
    let clock = || u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let one = || {
        let r = catch_unwind(AssertUnwindSafe(|| agent(&slot, &clock, pool)));
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
    // The trace goes to the segment before the result, so the leader finds it once it sees the result
    // (CONTRACT.md, S9).
    if let Some(tr) = slot.trace() {
        let at = crate::domain::arena(|| std::boxed::Box::into_raw(std::boxed::Box::new(tr)));
        segment::trace_slot(with_endpoint(|t| t.me())).store(at as u64, std::sync::atomic::Ordering::Release);
    }
    with_endpoint(|t| t.finish(word));
    finalize();
    process::exit(0)
}

/// Put this node's states and pool in its segment (CONTRACT.md, L3, M1). The node's builder is the
/// leader on the leader's node, and the host rank elsewhere: it runs the builders of the workers on
/// its node and the pool's, in arena mode, and writes their addresses to the segment's table. After
/// a world barrier, every other rank of the node adopts them. Builders of workers on other nodes are
/// dropped unrun. Collective over the world.
fn build_or_adopt(plan: &mut Plan, t: &Transport) {
    use std::sync::atomic::Ordering::{Acquire, Release};
    let builder = t.me() == t.lay().workers;
    let mine: Vec<usize> = (0..plan.workers.len()).collect();
    if builder {
        for &w in &mine {
            let b = plan.builders[w].take().expect("trame: the launch checked that every worker was set");
            crate::domain::arena(b);
            segment::state_slot(w as u32).store(plan.workers[w].addr() as u64, Release);
        }
        if let Some(b) = plan.pool_builder.take() {
            crate::domain::arena(b);
            let pool = plan.pool.as_ref().expect("trame: a pool builder belongs to a declared pool");
            segment::pool_slot().store(pool.addr() as u64, Release);
        }
    }
    mpi::traits::CommunicatorCollectives::barrier(&super::hosts::host().comm);
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

/// Run `f` on this rank's endpoint, under the rank's transport lock.
fn with_endpoint<R>(f: impl FnOnce(&Transport) -> R) -> R {
    worker::with_endpoint(f)
}

/// Agents per worker: this node's cores shared among its ranks, at most the worker's steps.
fn agents_for(obj: &dyn Erased, t: &Transport, cap: Option<usize>) -> usize {
    let here = t.lay().workers as usize + 1;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let fair = (cores / here).max(1);
    cap.unwrap_or(fair).min(fair).min(obj.steps().len()).max(1)
}

impl Backend for Selected {
    type Run = Leader;

    /// The ranks of the host agree on the plan, then the hosts on the routes between them (F3,
    /// cross.rs `connect`). The launch checks run here too, so that a host refused by them still
    /// takes part in the connect, and the hosts it shares routes with are refused with it.
    fn agree(plan: &Plan) -> Result<(), Refused> {
        let ranks = plan.workers.len() as u32 + 1;
        let here = agree(&super::hosts::host().comm, &encode_plan(plan), ranks).map_err(refused);
        let checked = here.and_then(|()| crate::launch::check(plan));
        let joined = super::cross::connect(plan, checked.is_ok());
        checked.and(joined)
    }

    fn launch(mut plan: Plan) -> Result<Leader, Refused> {
        let topo = topology(&plan)?;
        let role = open(&super::hosts::host().comm, &topo).map_err(refused)?;
        let t = match &role {
            Role::Leader(t) | Role::Worker(t) => t,
        };
        build_or_adopt(&mut plan, t);
        let table = entries(&plan);
        let l = local();
        l.entries = table;
        match role {
            Role::Leader(t) => {
                l.inbox = inbox(&l.entries, t.me());
                Ok(Leader::new(t, (core::mem::take(&mut plan.workers), plan.pool.take())))
            }
            Role::Worker(t) => {
                let me = t.me();
                l.inbox = inbox(&l.entries, me);
                let obj = plan.workers[me as usize].clone();
                let agents = agents_for(&*obj, &t, plan.agents);
                l.endpoint = Some(t);
                run_worker(obj, agents, plan.pool.clone(), plan.trace)
            }
        }
    }

    fn worker_send(to: End, ty: TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        worker::send(to, ty, name, bytes)
    }

    fn worker_recv(ty: TypeId, out: &mut [u8]) -> bool {
        worker::recv(ty, out)
    }

    fn worker_offer(ty: TypeId) -> bool {
        worker::offer(ty)
    }

    fn worker_route(to: End, ty: TypeId) -> Option<usize> {
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
        WorkerId::here(worker::me())
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

    fn leader_edit(run: &Leader, target: Target, f: &mut dyn FnMut()) -> bool {
        leader::edit(run, target, f)
    }

    fn leader_stop(run: &Leader) {
        leader::stop(run)
    }

    fn leader_join_traced(run: Leader, workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>) {
        let here = leader::local_workers(&run);
        let out = leader::join(run);
        let traces = (0..workers as u32)
            .map(|w| {
                let at = segment::trace_slot(w).load(std::sync::atomic::Ordering::Acquire);
                (here.contains(&w) && at != 0).then(|| {
                    // SAFETY: the worker wrote a boxed trace there before its result, and only this
                    // rank reads it, once (CONTRACT.md, S9).
                    *unsafe { std::boxed::Box::from_raw(at as *mut (Sched, Vec<Event>)) }
                })
            })
            .collect();
        (out, traces)
    }

    fn worker_edit_request() -> u64 {
        worker::edit_request()
    }

    fn worker_edit_held(held: bool) {
        worker::edit_held(held)
    }

    fn domain_alloc(layout: core::alloc::Layout) -> Option<*mut u8> {
        segment::mapped().then(|| segment::alloc(layout))
    }

    fn domain_contains(p: *mut u8) -> bool {
        segment::contains(p)
    }

    unsafe fn domain_dealloc(p: *mut u8, layout: core::alloc::Layout) {
        segment::free(p, layout)
    }

    fn leader_wait(run: &Leader, seen: u64) -> u64 {
        leader::wait(run, seen)
    }

    fn host() -> u32 {
        super::hosts::host().id
    }

    fn hosts() -> u32 {
        super::hosts::host().count
    }

    fn peer_send(_run: &Leader, to: u32, tag: u64, bytes: &[u8]) -> Result<(), Full> {
        super::hosts::send(to, tag, bytes);
        Ok(())
    }

    fn peer_recv(_run: &Leader, tag: u64, out: &mut [u8]) -> Option<u32> {
        super::hosts::recv(tag, out)
    }

    fn leader_join(run: Leader) -> Vec<Outcome> {
        leader::join(run)
    }
}
