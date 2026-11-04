//! The threads backend: workers in one process, each run by several agent threads at once, with no
//! baton. Steps of one worker whose fields do not conflict run truly in parallel; steps of different
//! workers too. Each agent runs the reference scheduler (worker.rs `agent`); service is nanoseconds,
//! so weights are shares of time (CONTRACT.md, S6).
//!
//! The number of agents per worker is the machine's parallelism (a test may lower it), never more than
//! the worker's steps. The routes are the shared-memory rings of
//! backends/local/routes.rs, under one lock, and every act that can change a wait condition moves the
//! run's counter `acts` and wakes every sleeper, as on seq.
//!
//! A worker's result is set once every one of its agents has returned, so `join` hands the leader a
//! state no agent touches. Deadlock is judged as on seq (stuck.rs there): no agent can move now, and
//! no deadline is pending.

extern crate std;

use crate::backend::{Backend, PoolRef, Target};
use crate::io::Full;
use crate::launch::{Plan, Refused};
use crate::local_routes::Routes;
use crate::run::{Outcome, panic_message};
use crate::sched::{Answer, Event, Sched};
use crate::time::ClockId;
use crate::topology::{LEADER, WorkerId};
use crate::worker::{Slot, agent};
use std::boxed::Box;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::string::String;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread_local;
use std::time::{Duration, Instant};
use std::vec::Vec;

pub struct Selected;

/// The unwinding payload of an agent that must stop without a result: the run ended, or the worker
/// deadlocked or was abandoned. Not a panic.
struct Abandoned;

/// One worker's agents, as the run counts them.
struct W {
    agents: usize,
    /// Agents that returned or unwound.
    finished: usize,
    /// Per agent: idle since the counter was `.0`, until `.1` (nanoseconds), or busy (None).
    idle: Vec<Option<(u64, Option<u64>)>>,
    answer: Option<Answer>,
    result: Option<Outcome>,
}

struct State {
    routes: Routes,
    acts: u64,
    pool: Option<PoolRef>,
    aborted: bool,
    panic: Option<(usize, String)>,
    workers: Vec<W>,
}

impl State {
    fn must_stop(&self, w: usize) -> bool {
        self.aborted || self.workers[w].result.is_some()
    }

    /// Whether worker `w` moves without time passing: an agent is busy, or idle with a change since.
    fn moves_now(&self, w: usize) -> bool {
        let x = &self.workers[w];
        if x.result.is_some() {
            return false;
        }
        let idle = x.idle.iter().flatten().count();
        x.agents - x.finished > idle || x.idle.iter().flatten().any(|(seen, _)| *seen != self.acts)
    }

    fn earliest_deadline(&self) -> Option<u64> {
        self.workers.iter().filter(|x| x.result.is_none()).flat_map(|x| x.idle.iter().flatten().filter_map(|(_, t)| *t)).min()
    }
}

struct Shared {
    st: Mutex<State>,
    changed: Condvar,
    epoch: Instant,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn park<'a>(&self, g: MutexGuard<'a, State>, dur: Option<Duration>) -> MutexGuard<'a, State> {
        match dur {
            Some(d) => self.changed.wait_timeout(g, d).unwrap_or_else(PoisonError::into_inner).0,
            None => self.changed.wait(g).unwrap_or_else(PoisonError::into_inner),
        }
    }

    fn nanos(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

pub struct Run {
    shared: Arc<Shared>,
    slots: Vec<Arc<Slot>>,
}

thread_local! {
    /// On an agent thread: the run, its worker and its index among the worker's agents.
    static CURRENT: RefCell<Option<(Arc<Shared>, usize, usize)>> = const { RefCell::new(None) };
}

fn current() -> (Arc<Shared>, usize, usize) {
    CURRENT.with(|c| c.borrow().clone()).expect("trame: this effect runs only in a step")
}

/// An act that can change a wait condition: move the counter and wake every sleeper.
fn acted(shared: &Shared, g: &mut State) {
    g.acts += 1;
    shared.changed.notify_all();
}

fn leader_check(g: MutexGuard<'_, State>) -> MutexGuard<'_, State> {
    if let Some((w, msg)) = &g.panic {
        let text = std::format!("trame: worker {w} panicked: {msg}");
        drop(g);
        panic!("{text}");
    }
    g
}

fn agent_main(shared: Arc<Shared>, w: usize, a: usize, slot: Arc<Slot>) {
    CURRENT.with(|c| *c.borrow_mut() = Some((shared.clone(), w, a)));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // Fixed for the launch: an edit writes it in place (CONTRACT.md, P2).
        let pool = shared.lock().pool.clone();
        let clock = || shared.nanos();
        agent(&slot, &clock, pool.as_deref().map(|p| p.any()))
    }));
    let mut g = shared.lock();
    g.workers[w].finished += 1;
    match outcome {
        Ok(answer) => g.workers[w].answer = Some(answer),
        Err(p) if p.is::<Abandoned>() => {}
        Err(p) => {
            g.aborted = true;
            if g.panic.is_none() {
                g.panic = Some((w, panic_message(&*p)));
            }
        }
    }
    let x = &mut g.workers[w];
    if x.finished == x.agents && x.result.is_none() {
        x.result = x.answer.map(|a| match a {
            Answer::Returned => Outcome::Returned,
            Answer::Stopped => Outcome::Stopped,
            Answer::Failed(_) => Outcome::Failed,
        });
    }
    acted(&shared, &mut g);
}

impl Backend for Selected {
    type Run = Run;

    fn launch(mut plan: Plan) -> Result<Run, Refused> {
        crate::launch::build_all(&mut plan);
        let routes = Routes::new(&plan.routes)?;
        let per = plan.agents.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
        let slots: Vec<Arc<Slot>> = plan.workers.into_iter().map(|w| Arc::new(Slot::new(w, plan.trace))).collect();
        let counts: Vec<usize> = slots.iter().map(|s| per.min(s.obj.steps().len()).max(1)).collect();
        let workers = counts
            .iter()
            .map(|&agents| W { agents, finished: 0, idle: std::vec![None; agents], answer: None, result: None })
            .collect();
        let st = State { routes, acts: 0, pool: plan.pool, aborted: false, panic: None, workers };
        let shared = Arc::new(Shared { st: Mutex::new(st), changed: Condvar::new(), epoch: Instant::now() });
        for (w, slot) in slots.iter().enumerate() {
            for a in 0..counts[w] {
                let (sh, slot) = (shared.clone(), slot.clone());
                if std::thread::Builder::new().spawn(move || agent_main(sh, w, a, slot)).is_err() {
                    let mut g = shared.lock();
                    g.aborted = true;
                    acted(&shared, &mut g);
                    return Err(Refused::Backend { reason: "could not start an agent thread" });
                }
            }
        }
        Ok(Run { shared, slots })
    }

    fn worker_me() -> WorkerId {
        WorkerId::here(current().1 as u32)
    }

    fn worker_send(to: WorkerId, ty: core::any::TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        let (shared, w, _) = current();
        let mut g = shared.lock();
        if g.must_stop(w) {
            drop(g);
            resume_unwind(Box::new(Abandoned));
        }
        let from = WorkerId::here(w as u32);
        let Some(i) = g.routes.find(from, to, ty) else {
            drop(g);
            panic!("trame: worker {from:?} has no route to {to:?} for {name}");
        };
        let answer = g.routes.push(i, bytes);
        acted(&shared, &mut g);
        answer
    }

    fn worker_recv(ty: core::any::TypeId, out: &mut [u8]) -> bool {
        let (shared, w, _) = current();
        let mut g = shared.lock();
        if g.must_stop(w) {
            drop(g);
            resume_unwind(Box::new(Abandoned));
        }
        let taken = g.routes.take(WorkerId::here(w as u32), ty, out);
        if taken {
            acted(&shared, &mut g);
        }
        taken
    }

    fn worker_offer(ty: core::any::TypeId) -> bool {
        let (shared, w, _) = current();
        let g = shared.lock();
        g.routes.all.iter().any(|r| r.to == WorkerId::here(w as u32) && r.ty == ty && r.len > 0)
    }

    fn worker_route(to: WorkerId, ty: core::any::TypeId) -> Option<usize> {
        let (shared, w, _) = current();
        let g = shared.lock();
        let from = WorkerId::here(w as u32);
        g.routes.all.iter().filter(|r| r.from == from).position(|r| r.to == to && r.ty == ty)
    }

    fn worker_room(route: usize) -> bool {
        let (shared, w, _) = current();
        let g = shared.lock();
        let from = WorkerId::here(w as u32);
        match g.routes.all.iter().filter(|r| r.from == from).nth(route) {
            Some(r) => r.lossy || r.len < r.cap,
            None => {
                drop(g);
                panic!("trame: worker {w} has no outgoing route {route} for this room check")
            }
        }
    }

    fn worker_epoch() -> u64 {
        current().0.lock().acts
    }

    fn worker_idle(until: Option<u64>, seen: u64) {
        let (shared, w, a) = current();
        let mut g = shared.lock();
        g.workers[w].idle[a] = Some((seen, until));
        // The leader may be judging whether the run is stuck.
        shared.changed.notify_all();
        loop {
            if g.must_stop(w) {
                g.workers[w].idle[a] = None;
                drop(g);
                resume_unwind(Box::new(Abandoned));
            }
            let now = shared.nanos();
            let due = until.is_some_and(|t| now >= t);
            if g.acts != seen || due {
                g.workers[w].idle[a] = None;
                return;
            }
            let left = until.map(|t| Duration::from_nanos(t - now));
            g = shared.park(g, left);
        }
    }

    fn worker_now() -> (ClockId, u64) {
        (ClockId::new(0), current().0.nanos())
    }

    fn worker_abandon() -> ! {
        resume_unwind(Box::new(Abandoned))
    }

    fn leader_send(run: &Run, to: WorkerId, ty: core::any::TypeId, name: &'static str, bytes: &[u8]) -> Result<(), Full> {
        let mut g = leader_check(run.shared.lock());
        let Some(i) = g.routes.find(LEADER, to, ty) else {
            drop(g);
            panic!("trame: the leader has no route to {to:?} for {name}");
        };
        let answer = g.routes.push(i, bytes);
        acted(&run.shared, &mut g);
        answer
    }

    fn leader_recv(run: &Run, ty: core::any::TypeId, out: &mut [u8]) -> bool {
        let mut g = leader_check(run.shared.lock());
        let taken = g.routes.take(LEADER, ty, out);
        if taken {
            acted(&run.shared, &mut g);
        }
        taken
    }

    fn leader_edit(run: &Run, target: Target, f: &mut dyn FnMut()) -> bool {
        drop(leader_check(run.shared.lock()));
        let done = match target {
            Target::Worker(w) => run.slots[w].edit(crate::sched::STATE, f),
            Target::Pool => {
                crate::worker::edit_pool(&run.slots, f);
                true
            }
        };
        let mut g = leader_check(run.shared.lock());
        acted(&run.shared, &mut g);
        done
    }

    fn leader_stop(run: &Run) {
        drop(leader_check(run.shared.lock()));
        for slot in &run.slots {
            slot.stop();
        }
        let mut g = run.shared.lock();
        acted(&run.shared, &mut g);
    }

    fn leader_wait(run: &Run, seen: u64) -> u64 {
        let mut g = leader_check(run.shared.lock());
        while g.acts == seen {
            let stuck = !(0..g.workers.len()).any(|w| g.moves_now(w)) && g.earliest_deadline().is_none();
            if stuck {
                drop(g);
                panic!("trame: deadlock: the leader waits and nothing can arrive");
            }
            g = run.shared.park(g, None);
            g = leader_check(g);
        }
        g.acts
    }

    fn leader_join(run: Run) -> Vec<Outcome> {
        let shared = run.shared;
        let mut g = leader_check(shared.lock());
        while g.workers.iter().any(|x| x.result.is_none()) {
            g = leader_check(g);
            if (0..g.workers.len()).any(|w| g.moves_now(w)) {
                g = shared.park(g, None);
                continue;
            }
            if let Some(t) = g.earliest_deadline() {
                let left = t.saturating_sub(shared.nanos());
                g = shared.park(g, Some(Duration::from_nanos(left.max(1))));
                continue;
            }
            for x in g.workers.iter_mut().filter(|x| x.result.is_none()) {
                x.result = Some(Outcome::Deadlock);
            }
            shared.changed.notify_all();
        }
        let out = g.workers.iter().map(|x| x.result.expect("join waits for every result")).collect();
        // Every agent of a deadlocked worker unwinds from its idle; wait for them, so that no agent
        // holds the state when the leader takes it.
        while g.workers.iter().any(|x| x.finished < x.agents) {
            g = shared.park(g, Some(Duration::from_millis(10)));
        }
        out
    }

    fn leader_join_traced(run: Run, workers: usize) -> (Vec<Outcome>, Vec<Option<(Sched, Vec<Event>)>>) {
        let slots = run.slots.clone();
        let out = Self::leader_join(run);
        (out, (0..workers).map(|w| slots[w].trace()).collect())
    }
}
