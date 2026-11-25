// The leader's side: the Run the backend hands the host, and its sends, takes and join. The leader is
// rank `workers`; its endpoint is a Transport, which needs `&mut` for a send or a take. The trait
// gives `&Run`, so the endpoint sits in an UnsafeCell. The leader's calls run on the rank's main
// thread one at a time, and none calls another, so no two `&mut` to it are ever alive together.

extern crate std;

use core::any::TypeId;
use core::cell::UnsafeCell;
use std::vec::Vec;

use crate::io::Full;
use crate::run::Outcome;
use crate::topology::{End, WorkerId};

use crate::backend::Target;
use crate::backends::mpi::memory::layout::{
    K_EDIT, K_HELD, K_OUTCOME, OUT_DEADLOCK, OUT_FAILED, OUT_RETURNED, OUT_STOPPED,
};
use crate::backends::mpi::setup::plan::{self, Entry, rank_of};
use crate::backends::mpi::setup::state::{finalize, local};
use crate::backends::mpi::types::{Answer, Received};
use crate::backends::mpi::{RouteId, Transport};
use std::sync::atomic::Ordering::SeqCst;

impl Transport {
    /// The leader's endpoint is dropped by a panic before `join`. A panic ends the run (INTERFACE.md 5, Abort): the
    /// workers unwind, and `end_run` closes the run. The panic hook has printed the message. `end_run` runs
    /// even when an abort is already recorded: one may have arrived from a
    /// worker before the leader's panic, and then nothing else ends the run (src/backends/mpi/proofs, `MCAbortEarly`).
    /// `end_run` may run twice: an object already gone is not an error. After `MPI_Finalize`
    /// (`fail_if_aborted` ended the run) there is nothing left to do.
    pub(crate) fn leader_panicked(&self) {
        let mut done: i32 = 0;
        // SAFETY: MPI_Finalized takes one out-pointer and may be called at any time.
        unsafe { mpi::ffi::MPI_Finalized(&mut done) };
        if done != 0 {
            return;
        }
        if !self.aborted() {
            self.record_abort(self.me(), b"the leader panicked before join");
        }
        self.end_run();
    }
}

/// A launched run, as the leader holds it: the leader's endpoint, and the workers and pool it built.
/// Their blocks are in the segment, where the workers use them; the leader process owns them, and a
/// block is freed when its last `Arc` goes. The program may drop its `Worker` and `Pool` handles
/// right after `set`, so the run keeps the plan's references until `join` (CONTRACT.md, M7). A run
/// dropped without `join` leaves its workers running, so it leaks them instead of freeing.
pub struct Leader(UnsafeCell<Transport>, core::mem::ManuallyDrop<Built>);

/// What the leader built, kept alive for the run (see `Leader`).
pub(crate) type Built = (Vec<std::sync::Arc<dyn crate::worker::Erased>>, Option<crate::backend::PoolRef>);

impl Leader {
    pub(crate) fn new(t: Transport, built: Built) -> Leader {
        Leader(UnsafeCell::new(t), core::mem::ManuallyDrop::new(built))
    }

    /// The endpoint. Each call is one leader operation, which ends before the next begins.
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut Transport {
        // SAFETY: see the module comment. The leader is one thread, and no operation holds this
        // reference across another operation.
        unsafe { &mut *self.0.get() }
    }
}

/// The route the leader sends on to `to` with message type `ty`. Panics when there is none.
fn route(entries: &[Entry], t: &Transport, to: WorkerId, ty: TypeId, name: &str) -> RouteId {
    let to_rank = rank_of(End::Worker(to), t.lay().workers);
    plan::find(entries, plan::here(t.me()), to_rank, ty)
        .unwrap_or_else(|| panic!("trame: the leader has no route of {name} to {to:?}"))
}

pub(crate) fn send(run: &Leader, to: WorkerId, ty: TypeId, name: &str, bytes: &[u8]) -> Result<(), Full> {
    let t = run.get();
    t.fail_if_aborted();
    let id = route(&local().entries, t, to, ty, name);
    match t.send(id, bytes) {
        Ok(Answer::Ok) => Ok(()),
        Ok(Answer::Full) => Err(Full),
        Err(e) => panic!("trame: leader send of {name}: {e}"),
    }
}

pub(crate) fn recv(run: &Leader, ty: TypeId, out: &mut [u8]) -> bool {
    let t = run.get();
    t.fail_if_aborted();
    let owned = plan::owned(&local().inbox, ty);
    match t.recv(owned, out) {
        Ok(Received::Message(_)) => true,
        Ok(Received::Empty) => false,
        Err(e) => panic!("trame: leader recv: {e}"),
    }
}

/// Stop the run: workers see STOP at their next scheduling round (results.rs `stop`).
pub(crate) fn stop(run: &Leader) {
    run.get().fail_if_aborted();
    run.get().stop()
}

/// The host's workers: their states and the pool are in its segment (segment.rs).
pub(crate) fn local_workers(run: &Leader) -> Vec<u32> {
    (0..run.get().lay().workers).collect()
}

/// A leader edit (CONTRACT.md, S4, P2), by the handshake of layout.rs: ask each worker concerned to
/// hold the fields (EDIT) and wake it; wait until each holds them (HELD) or has a result; run `f` in
/// this process, on the segment; withdraw the request and wait until each gave them back or has a
/// result. A worker edit answers false, without running `f`, when the worker has a result before it
/// holds.
pub(crate) fn edit(run: &Leader, target: Target, f: &mut dyn FnMut()) -> bool {
    let t = run.get();
    t.fail_if_aborted();
    let here = local_workers(run);
    let (ws, fields): (Vec<u32>, u64) = match target {
        Target::Worker(w) => (std::vec![w as u32], crate::sched::STATE),
        Target::Pool => (here, crate::sched::POOL),
    };
    let word = |k: usize, w: u32| t.win().word(t.lay().per_worker(k, w));
    let ended = |w: u32| word(K_OUTCOME, w).load(SeqCst) != 0;
    let wait_all = |done: &dyn Fn(u32) -> bool| loop {
        let e = t.leader_epoch();
        if ws.iter().all(|&w| done(w) || ended(w)) {
            break;
        }
        t.fail_if_aborted();
        t.leader_wait(e);
    };
    for &w in &ws {
        word(K_EDIT, w).store(fields, SeqCst);
        t.bump(w);
    }
    wait_all(&|w| word(K_HELD, w).load(SeqCst) == 1);
    let run_it = match target {
        Target::Worker(w) => !ended(w as u32),
        Target::Pool => true,
    };
    if run_it {
        f();
    }
    for &w in &ws {
        word(K_EDIT, w).store(0, SeqCst);
        t.bump(w);
    }
    wait_all(&|w| word(K_HELD, w).load(SeqCst) == 0);
    t.fail_if_aborted();
    run_it
}

/// Block until the leader's epoch differs from `seen`, and answer the new value (epoch.rs). Panics
/// when the detector shows that nothing can arrive (detect.rs), as seq does (INTERFACE.md 5, Deadlock).
pub(crate) fn wait(run: &Leader, seen: u64) -> u64 {
    run.get().fail_if_aborted();
    let stuck = || {
        if crate::backends::mpi::roles::detect::round(false) == crate::backends::mpi::roles::detect::Verdict::Stuck {
            panic!("trame: deadlock: the leader waits and nothing can arrive");
        }
    };
    // With peers, a peer message is a change too, and it arrives through MPI, not through the window: MPI offers no
    // descriptor to wait on, so a leader that waits for one polls the peer channel (`hosts::drain`) between naps. The
    // nap is on the leader's epoch futex with a bound (`crate::leader::NAP_NANOS`), so a worker's push wakes the
    // leader at once and a peer message is noticed within that bound. Workers never sleep: their own loop polls its incoming messages, and
    // only a leader naps between polls (INTERFACE.md 6; src/backends/README.md, The traits' waits).
    let e = if crate::backends::mpi::transport::hosts::host().count > 1 {
        // The seen word is the epoch in the low half and the count of peer arrivals in the high half.
        loop {
            crate::backends::mpi::transport::hosts::drain();
            let e = run.get().leader_epoch()
                | (crate::backends::mpi::transport::hosts::host().arrivals & 0xffff_ffff) << 32;
            if e != seen {
                break e;
            }
            stuck();
            run.get().leader_nap(e as u32, crate::leader::NAP_NANOS);
        }
    } else {
        loop {
            let e = run.get().leader_epoch();
            if e != seen {
                break e;
            }
            stuck();
            run.get().leader_nap(e as u32, crate::backends::mpi::roles::detect::pause());
        }
    };
    run.get().fail_if_aborted();
    e
}

/// Wait for every worker's result (see `join` in results.rs), in declaration order. Then finalize:
/// every outcome is read, so no result can depend on this rank's exit. An aborted run panics instead.
pub(crate) fn join(run: Leader) -> Vec<Outcome> {
    let words = run.get().join();
    // The launch's memory share is fixed for its run; a later launch fixes its own.
    crate::system::unfix();
    run.get().fail_if_aborted();
    // Peer messages this leader sent are delivered before it ends (transport/hosts.rs `flush`).
    crate::backends::mpi::transport::hosts::flush();
    let out = words
        .into_iter()
        .map(|w| match w {
            OUT_RETURNED => Outcome::Returned,
            OUT_DEADLOCK => Outcome::Deadlock,
            OUT_STOPPED => Outcome::Stopped,
            OUT_FAILED => Outcome::Failed,
            other => panic!("trame: unknown outcome word {other}"),
        })
        .collect();
    run.get().end_run();
    finalize();
    // Every worker has its result: no step uses a block now.
    drop(core::mem::ManuallyDrop::into_inner(run.1));
    out
}
