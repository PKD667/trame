//! Operations of the sequential backend. Each worker is an OS thread. Exactly one agent runs at a
//! time: the holder of the baton. A worker keeps the baton between its effects and hands it on at
//! each effect; the leader runs outside the baton, under the same Mutex.

extern crate std;

use super::state::{Abandoned, Phase, Route, Shared, State};
use super::Selected;
use crate::backend::Backend;
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::Outcome;
use crate::time::ClockId;
use crate::topology::{Context, WorkerId, LEADER};
use std::boxed::Box;
use std::cell::RefCell;
use std::thread_local;
use std::vec;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use std::vec::Vec;

/// One launched run, as the leader holds it.
pub struct Run {
    shared: Arc<Shared>,
}

thread_local! {
    /// On a worker thread: the run it belongs to, and its index. Empty on the leader's thread.
    static CURRENT: RefCell<Option<(Arc<Shared>, usize)>> = const { RefCell::new(None) };
}

fn current() -> (Arc<Shared>, usize) {
    CURRENT.with(|c| c.borrow().clone()).expect("trame: this effect runs only in a worker")
}

/// Block until worker `me` holds the baton. Unwinds instead when the worker must stop: the run
/// was aborted, or the worker already has a result.
fn wait_turn<'a>(shared: &'a Shared, mut st: MutexGuard<'a, State>, me: usize) -> MutexGuard<'a, State> {
    while st.turn != Some(me) && !st.must_stop(me) {
        st = shared.park(st);
    }
    if st.must_stop(me) {
        drop(st);
        resume_unwind(Box::new(Abandoned));
    }
    st
}

/// One worker effect. Holding the baton, apply `op` to the state, hand the baton on, and wait for
/// the next turn. Returns the answer of `op`.
fn effect<T>(op: impl FnOnce(&mut State, usize) -> T) -> T {
    turn_effect(op, true)
}

/// As `effect`. `bump` is false for a read that changes no state, so that it wakes no idle worker:
/// a read that woke them would make two waiting workers wake each other forever.
fn turn_effect<T>(op: impl FnOnce(&mut State, usize) -> T, bump: bool) -> T {
    let (shared, me) = current();
    let mut st = shared.lock();
    assert_eq!(st.turn, Some(me), "trame: a worker effect ran without the baton");
    let answer = op(&mut st, me);
    if bump {
        st.acts += 1;
    }
    st.advance(me);
    shared.changed.notify_all();
    drop(wait_turn(&shared, st, me));
    answer
}

/// The body of a worker's thread. A worker that returns hands the baton on. A panic aborts the run.
fn worker_main(shared: Arc<Shared>, me: usize, f: Box<dyn Fn(&Context) + Send>, ctx: Context) {
    CURRENT.with(|c| *c.borrow_mut() = Some((shared.clone(), me)));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        drop(wait_turn(&shared, shared.lock(), me));
        f(&ctx);
    }));
    let mut st = shared.lock();
    match outcome {
        Ok(()) => {
            st.phase[me] = Phase::Done(Outcome::Returned);
            st.advance(me);
            // A result is an act the leader waits for (leader_wait).
            st.acts += 1;
        }
        Err(payload) if payload.is::<Abandoned>() => {}
        Err(_) => {
            st.abort_all();
            st.acts += 1;
        }
    }
    shared.changed.notify_all();
}

impl Backend for Selected {
    type Run = Run;

    fn launch(plan: Plan) -> Result<Run, Refused> {
        let n = plan.workers.len();
        let ids: Vec<WorkerId> = (0..n).map(|i| WorkerId(i as u32)).collect();
        let mut routes = Vec::with_capacity(plan.routes.len());
        for r in &plan.routes {
            let bytes = match r.cap.checked_mul(r.size) {
                Some(bytes) => bytes,
                None => return Err(Refused::Backend { reason: "a route's frames do not fit in memory" }),
            };
            routes.push(Route {
                from: r.from,
                to: r.to,
                ty: r.ty,
                cap: r.cap,
                size: r.size,
                lossy: r.lossy,
                buf: vec![0; bytes],
                head: 0,
                len: 0,
            });
        }
        let state = State {
            phase: vec![Phase::Ready; n],
            turn: None,
            aborted: false,
            routes,
            cursor: 0,
            publication: Arc::from(plan.publication),
            pending: None,
            epoch: Instant::now(),
            acts: 0,
        };
        let shared = Arc::new(Shared { state: Mutex::new(state), changed: Condvar::new() });
        for (i, f) in plan.workers.into_iter().enumerate() {
            let ctx = Context::new(WorkerId(i as u32), ids.clone());
            let sh = shared.clone();
            if std::thread::Builder::new().spawn(move || worker_main(sh, i, f, ctx)).is_err() {
                shared.lock().abort_all();
                shared.changed.notify_all();
                return Err(Refused::Backend { reason: "could not start a worker thread" });
            }
        }
        // Every thread exists; only now may the first worker run.
        shared.lock().turn = (n > 0).then_some(0);
        shared.changed.notify_all();
        Ok(Run { shared })
    }

    fn worker_send(to: WorkerId, ty: core::any::TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        effect(|st, me| {
            let from = WorkerId(me as u32);
            match st.route(from, to, ty) {
                Some(i) => st.push(i, bytes),
                None => panic!("trame: worker {from:?} has no route to {to:?} for {name}"),
            }
        })
    }

    fn worker_recv(ty: core::any::TypeId, out: &mut [u8]) -> bool {
        effect(|st, me| st.take(WorkerId(me as u32), ty, out))
    }

    fn worker_offer(ty: core::any::TypeId) -> bool {
        let (shared, me) = current();
        let st = shared.lock();
        st.routes.iter().any(|r| r.to == WorkerId(me as u32) && r.ty == ty && r.len > 0)
    }

    fn worker_route(to: WorkerId, ty: core::any::TypeId) -> Option<usize> {
        let (shared, me) = current();
        let st = shared.lock();
        let from = WorkerId(me as u32);
        st.routes.iter().filter(|r| r.from == from).position(|r| r.to == to && r.ty == ty)
    }

    fn worker_room(route: usize) -> bool {
        let (shared, me) = current();
        let st = shared.lock();
        let from = WorkerId(me as u32);
        match st.routes.iter().filter(|r| r.from == from).nth(route) {
            Some(r) => r.lossy || r.len < r.cap,
            None => panic!("trame: worker {me} has no outgoing route {route} for this room check"),
        }
    }

    fn worker_epoch() -> u64 {
        current().0.lock().acts
    }

    fn worker_idle(until: Option<u64>, seen: u64) {
        let (shared, me) = current();
        let mut st = shared.lock();
        assert_eq!(st.turn, Some(me), "trame: an idle worker ran without the baton");
        // Idle: give the baton up. Nothing runs until a leader acts, or the deadline comes.
        st.phase[me] = Phase::Idle(seen, until);
        st.advance(me);
        shared.changed.notify_all();
        loop {
            if st.must_stop(me) {
                drop(st);
                resume_unwind(Box::new(Abandoned));
            }
            let elapsed = u64::try_from(st.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
            let due = until.is_some_and(|t| elapsed >= t);
            if (st.acts != seen || due) && st.turn.is_none() {
                st.turn = Some(me);
                st.phase[me] = Phase::Ready;
                shared.changed.notify_all();
                return;
            }
            st = match until {
                Some(t) if !due => shared.park_timeout(st, Duration::from_nanos(t - elapsed)),
                _ => shared.park(st),
            };
        }
    }

    fn worker_now() -> (ClockId, u64) {
        turn_effect(|st, _| {
            let nanos = u64::try_from(st.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
            (ClockId::new(0), nanos)
        }, false)
    }

    fn worker_barrier() -> Arc<[u8]> {
        effect(|st, me| {
            st.phase[me] = Phase::Blocked;
            if st.phase.iter().all(|p| *p == Phase::Blocked) {
                st.fire();
            }
        });
        // Read after the wait: the publication changes only when the barrier fires, and no worker
        // can reach the next barrier before this one has read it. The clone shares the bytes.
        current().0.lock().publication.clone()
    }

    fn leader_send(run: &Run, to: WorkerId, ty: core::any::TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        let mut st = run.shared.lock();
        let answer = match st.route(LEADER, to, ty) {
            Some(i) => st.push(i, bytes),
            None => panic!("trame: the leader has no route to {to:?} for {name}"),
        };
        st.acts += 1;
        run.shared.changed.notify_all();
        answer
    }

    fn leader_recv(run: &Run, ty: core::any::TypeId, out: &mut [u8]) -> bool {
        // A take frees room on a route, so it wakes a worker that waits for room.
        let mut st = run.shared.lock();
        let taken = st.take(LEADER, ty, out);
        if taken {
            st.acts += 1;
            run.shared.changed.notify_all();
        }
        taken
    }

    fn leader_publish(run: &Run, bytes: &[u8]) {
        let mut st = run.shared.lock();
        st.pending = Some(Arc::from(bytes));
        st.acts += 1;
        run.shared.changed.notify_all();
    }

    fn leader_wait(run: &Run, seen: u64) -> u64 {
        // Every act bumps `acts` and wakes the leader (turn_effect, the leader's ops, a result).
        let mut st = run.shared.lock();
        while st.acts == seen {
            st = run.shared.park(st);
        }
        st.acts
    }

    fn leader_join(run: Run) -> Vec<Outcome> {
        let shared = run.shared;
        let mut st = shared.lock();
        // The leader sends nothing after join. A worker that is still able to move is waited for.
        // When none is, and no idle worker has seen a change since it went idle, the run is stuck
        // (spec.md 5, Deadlock): every agent without a result gets deadlock. An idle worker with a
        // deadline is not stuck: it moves when time passes, so join waits for the earliest one.
        while st.phase.iter().any(|p| !matches!(p, Phase::Done(_))) {
            let ready = st.phase.iter().any(|p| *p == Phase::Ready);
            let pending = st.phase.iter().any(|p| matches!(p, Phase::Idle(seen, _) if *seen != st.acts));
            if ready || pending {
                st = shared.park(st);
                continue;
            }
            let deadline = st.phase.iter().filter_map(|p| match p {
                Phase::Idle(_, Some(t)) => Some(*t),
                _ => None,
            }).min();
            if let Some(t) = deadline {
                let elapsed = u64::try_from(st.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                // Past the deadline, the worker wakes itself and notifies: a plain wait is enough.
                st = match t.checked_sub(elapsed) {
                    Some(left) if left > 0 => shared.park_timeout(st, Duration::from_nanos(left)),
                    _ => shared.park(st),
                };
                continue;
            }
            for p in st.phase.iter_mut().filter(|p| matches!(p, Phase::Blocked | Phase::Idle(..))) {
                *p = Phase::Done(Outcome::Deadlock);
            }
            shared.changed.notify_all();
        }
        st.phase
            .iter()
            .map(|p| match p {
                Phase::Done(outcome) => *outcome,
                _ => unreachable!("join waits until every worker has a result"),
            })
            .collect()
    }
}
