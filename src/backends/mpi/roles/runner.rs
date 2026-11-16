// A worker rank's life after `launch`: its agents, its result, the end of the process. Rank i < W is worker i.
// The rank runs its state in place with several agents and never returns to the host: it records its outcome in
// the window and ends the process (CONTRACT.md, L3, S3).

extern crate std;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process;
use std::sync::Arc;
use std::vec::Vec;

use crate::backend::PoolRef;
use crate::run::panic_message;
use crate::sched::Answer;
use crate::worker::{Erased, Slot, agent};

use crate::backends::mpi::Transport;
use crate::backends::mpi::memory::layout::{OUT_ABORT, OUT_DEADLOCK, OUT_FAILED, OUT_RETURNED, OUT_STOPPED};
use crate::backends::mpi::memory::segment;
use crate::backends::mpi::roles::worker;
use crate::backends::mpi::setup::state::finalize;

/// Run this worker: `agents` agents of the reference scheduler (worker.rs), the main thread one of
/// them. A panic in a step records the abort at once (results.rs `abort`), which wakes every sleeping
/// agent of the run so it unwinds; an abort that reached this worker first records only its own result.
/// An agent the leader showed stuck unwinds with `Deadlocked` (worker.rs), and then the result is
/// OUT_DEADLOCK. Otherwise the worker's answer is its result. The result is recorded before anything waits on a
/// peer: then MPI_Finalize (state.rs), which waits for every rank, and then the process ends.
pub(crate) fn run_worker(obj: Arc<dyn Erased>, agents: usize, pool: Option<PoolRef>, trace: bool) -> ! {
    worker::fast_init();
    let slot = Slot::new(obj, trace);
    let pool = pool.as_deref().map(|p| p.any());
    let start = std::time::Instant::now();
    let clock = || u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let stuck = std::sync::atomic::AtomicBool::new(false);
    let one = || {
        let r = catch_unwind(AssertUnwindSafe(|| agent(&slot, &clock, pool)));
        if let Err(p) = &r
            && p.is::<worker::Deadlocked>()
        {
            stuck.store(true, std::sync::atomic::Ordering::Relaxed);
        } else if let Err(p) = &r
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
    // The scope joined every agent, so `stuck` is final.
    let deadlocked = stuck.load(std::sync::atomic::Ordering::Relaxed) && !with_endpoint(|t| t.aborted());
    let word = match answers.iter().flatten().next() {
        _ if deadlocked => OUT_DEADLOCK,
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

/// Run `f` on this rank's endpoint, under the rank's transport lock.
fn with_endpoint<R>(f: impl FnOnce(&Transport) -> R) -> R {
    worker::with_endpoint(f)
}

/// Agents per worker: this node's cores shared among its ranks, at most the worker's steps.
pub(crate) fn agents_for(obj: &dyn Erased, t: &Transport, cap: Option<usize>) -> usize {
    let here = t.lay().workers as usize + 1;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let fair = (cores / here).max(1);
    cap.unwrap_or(fair).min(fair).min(obj.steps().len()).max(1)
}
