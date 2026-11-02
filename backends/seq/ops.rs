//! Operations of the sequential backend. Each worker is one OS thread, its one agent, running the
//! reference scheduler (worker.rs `agent`) with service 1 per step: so seq follows `Sched::choose`
//! exactly (backend.rs, S6). Exactly one agent runs at a time: the holder of the baton. A worker keeps
//! the baton between its effects and hands it on at each effect; the leader runs outside the baton,
//! under the same Mutex.

extern crate std;

use super::Selected;
use super::limit;
use super::state::{Abandoned, Phase, Shared, State};
use crate::local_routes::Routes;
use crate::backend::{Backend, Target};
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::run::{Outcome, panic_message};
use crate::sched::{Answer, Event, Sched};
use crate::time::ClockId;
use crate::topology::{LEADER, WorkerId};
use crate::worker::{Slot, agent};
use std::boxed::Box;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread_local;
use std::time::{Duration, Instant};
use std::vec;
use std::vec::Vec;

/// One launched run, as the leader holds it.
pub struct Run {
    shared: Arc<Shared>,
    slots: Vec<Arc<Slot>>,
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

/// The leader's check, before each of its calls: an aborted run panics the leader, naming the worker
/// and its message (spec.md 5, Abort). The guard is released before the panic, so the Mutex is not
/// poisoned for the leader's own calls.
fn leader_check(st: MutexGuard<'_, State>) -> MutexGuard<'_, State> {
    if let Some((w, msg)) = &st.panic {
        let text = std::format!("trame: worker {w} panicked: {msg}");
        drop(st);
        panic!("{text}");
    }
    st
}

/// The body of a worker's thread: its one agent. A worker with a result hands the baton on. A panic
/// aborts the run.
fn worker_main(shared: Arc<Shared>, me: usize, slot: Arc<Slot>) {
    CURRENT.with(|c| *c.borrow_mut() = Some((shared.clone(), me)));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        drop(wait_turn(&shared, shared.lock(), me));
        // Fixed for the launch: an edit writes it in place (backend.rs, P2).
        let pool = shared.lock().pool.clone();
        agent(&slot, &|| 0, pool.as_deref())
    }));
    let mut st = shared.lock();
    match outcome {
        Ok(answer) => {
            st.phase[me] = Phase::Done(match answer {
                Answer::Returned => Outcome::Returned,
                Answer::Stopped => Outcome::Stopped,
                Answer::Failed(_) => Outcome::Failed,
            });
            st.advance(me);
            // A result is an act the leader waits for (leader_wait).
            st.acts += 1;
        }
        Err(payload) if payload.is::<Abandoned>() => {}
        Err(payload) => {
            st.abort_all(Some((me, panic_message(&*payload))));
            st.acts += 1;
        }
    }
    shared.changed.notify_all();
}

impl Backend for Selected {
    type Run = Run;

    fn launch(mut plan: Plan) -> Result<Run, Refused> {
        crate::launch::build_all(&mut plan);
        let n = plan.workers.len();
        let routes = Routes::new(&plan.routes)?;
        let limit = limit::setting().map_err(|reason| Refused::Backend { reason })?;
        let state = State {
            phase: vec![Phase::Ready; n],
            turn: None,
            aborted: false,
            panic: None,
            routes,
            pool: plan.pool,
            epoch: Instant::now(),
            acts: 0,
            steps: 0,
            limit,
        };
        let shared = Arc::new(Shared { state: Mutex::new(state), changed: Condvar::new() });
        let slots: Vec<Arc<Slot>> = plan.workers.into_iter().map(|w| Arc::new(Slot::new(w, plan.trace))).collect();
        for (i, slot) in slots.iter().enumerate() {
            let (sh, slot) = (shared.clone(), slot.clone());
            if std::thread::Builder::new().spawn(move || worker_main(sh, i, slot)).is_err() {
                shared.lock().abort_all(None);
                shared.changed.notify_all();
                return Err(Refused::Backend { reason: "could not start a worker thread" });
            }
        }
        // Every thread exists; only now may the first worker run.
        shared.lock().turn = (n > 0).then_some(0);
        shared.changed.notify_all();
        Ok(Run { shared, slots })
    }

    fn worker_me() -> WorkerId {
        WorkerId(current().1 as u32)
    }

    fn worker_send(to: WorkerId, ty: core::any::TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        effect(|st, me| {
            let from = WorkerId(me as u32);
            match st.routes.find(from, to, ty) {
                Some(i) => st.routes.push(i, bytes),
                None => panic!("trame: worker {from:?} has no route to {to:?} for {name}"),
            }
        })
    }

    fn worker_recv(ty: core::any::TypeId, out: &mut [u8]) -> bool {
        effect(|st, me| st.routes.take(WorkerId(me as u32), ty, out))
    }

    fn worker_offer(ty: core::any::TypeId) -> bool {
        let (shared, me) = current();
        let st = shared.lock();
        st.routes.all.iter().any(|r| r.to == WorkerId(me as u32) && r.ty == ty && r.len > 0)
    }

    fn worker_route(to: WorkerId, ty: core::any::TypeId) -> Option<usize> {
        let (shared, me) = current();
        let st = shared.lock();
        let from = WorkerId(me as u32);
        st.routes.all.iter().filter(|r| r.from == from).position(|r| r.to == to && r.ty == ty)
    }

    fn worker_room(route: usize) -> bool {
        let (shared, me) = current();
        let st = shared.lock();
        let from = WorkerId(me as u32);
        match st.routes.all.iter().filter(|r| r.from == from).nth(route) {
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

    fn worker_abandon() -> ! {
        resume_unwind(Box::new(Abandoned))
    }

    fn worker_now() -> (ClockId, u64) {
        turn_effect(
            |st, _| {
                let nanos = u64::try_from(st.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                (ClockId::new(0), nanos)
            },
            false,
        )
    }

    fn leader_send(
        run: &Run,
        to: WorkerId,
        ty: core::any::TypeId,
        name: &'static str,
        bytes: &[u8],
    ) -> Result<(), Full> {
        let mut st = leader_check(run.shared.lock());
        let answer = match st.routes.find(LEADER, to, ty) {
            Some(i) => st.routes.push(i, bytes),
            None => panic!("trame: the leader has no route to {to:?} for {name}"),
        };
        st.acts += 1;
        run.shared.changed.notify_all();
        answer
    }

    fn leader_recv(run: &Run, ty: core::any::TypeId, out: &mut [u8]) -> bool {
        // A take frees room on a route, so it wakes a worker that waits for room.
        let mut st = leader_check(run.shared.lock());
        let taken = st.routes.take(LEADER, ty, out);
        if taken {
            st.acts += 1;
            run.shared.changed.notify_all();
        }
        taken
    }

    fn leader_edit(run: &Run, target: Target, f: &mut dyn FnMut()) -> bool {
        drop(leader_check(run.shared.lock()));
        // Outside the run's Mutex: the edit waits for the worker's running step, which needs the baton.
        let done = match target {
            Target::Worker(w) => run.slots[w].edit(crate::sched::STATE, f),
            Target::Pool => {
                crate::worker::edit_pool(&run.slots, f);
                true
            }
        };
        let mut st = leader_check(run.shared.lock());
        st.acts += 1;
        run.shared.changed.notify_all();
        done
    }

    fn leader_stop(run: &Run) {
        drop(leader_check(run.shared.lock()));
        for slot in &run.slots {
            slot.stop();
        }
        let mut st = run.shared.lock();
        st.acts += 1;
        run.shared.changed.notify_all();
    }

    fn leader_wait(run: &Run, seen: u64) -> u64 {
        // Every act bumps `acts` and wakes the leader (turn_effect, the leader's ops, a result).
        let mut st = leader_check(run.shared.lock());
        while st.acts == seen {
            // Stuck: this call can never return (spec.md 5, Deadlock). The guard is released first.
            if st.leader_stuck() {
                drop(st);
                panic!("trame: deadlock: the leader waits and nothing can arrive");
            }
            st = run.shared.park(st);
            st = leader_check(st);
        }
        st.acts
    }

    fn leader_join_traced(run: Run, workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>) {
        let slots = run.slots.clone();
        let out = Self::leader_join(run);
        (out, (0..workers).map(|w| slots[w].trace()).collect())
    }

    fn leader_join(run: Run) -> Vec<Outcome> {
        let shared = run.shared;
        let mut st = leader_check(shared.lock());
        // The leader sends nothing after join. A worker that is still able to move is waited for.
        // When none is, and no idle worker has seen a change since it went idle, the run is stuck
        // (spec.md 5, Deadlock): every agent without a result gets deadlock. An idle worker with a
        // deadline is not stuck: it moves when time passes, so join waits for the earliest one.
        while st.phase.iter().any(|p| !matches!(p, Phase::Done(_))) {
            st = leader_check(st);
            if st.worker_moves_now() {
                st = shared.park(st);
                continue;
            }
            if let Some(t) = st.earliest_deadline() {
                let elapsed = u64::try_from(st.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                // Past the deadline, the worker wakes itself and notifies: a plain wait is enough.
                st = match t.checked_sub(elapsed) {
                    Some(left) if left > 0 => shared.park_timeout(st, Duration::from_nanos(left)),
                    _ => shared.park(st),
                };
                continue;
            }
            for p in st.phase.iter_mut().filter(|p| matches!(p, Phase::Idle(..))) {
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
