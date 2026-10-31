//! A worker is a state and its steps (spec.md 5.3). This file has three parts:
//!
//! 1. `State`, which `#[trame::state]` implements: the steps' read and write sets, and a `run` that
//!    calls step i with its fields. The state lives in `Core`, behind a raw pointer: steps that hold
//!    disjoint fields run at once, so no `&mut S` to the whole state may exist while they run.
//! 2. `Erased`: the worker as a backend sees it, without its type.
//! 3. `Slot` and `agent`: the reference scheduler, the model of sched.rs driven by real calls. A
//!    backend may use it (seq does) or schedule by itself; either way it keeps the contract of
//!    backend.rs, and conformance replays its trace against sched.rs.

extern crate std;

use crate::backend::{Backend, Selected};
use crate::cells::{Owning, Tags};
use crate::sched::{Answer, Ended, Event, Now, Sched, St, Weight};
use crate::status::{Kind, Status, Waits};
use crate::time;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::{Any, TypeId};
use core::cell::UnsafeCell;
use core::mem::ManuallyDrop;
use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// One step of a state, as `#[trame::step]` describes it.
#[derive(Clone, Copy, Debug)]
pub struct StepInfo {
    pub name: &'static str,
    /// Bit k: the step takes field k as `&`.
    pub reads: u64,
    /// Bit k: the step takes field k as `&mut`.
    pub writes: u64,
    /// Its weight until a status sets another.
    pub weight: Weight,
}

/// A worker's state with its steps. Implemented by `#[trame::state]`; not by hand.
///
/// # Safety
/// `STEPS[i]` lists exactly the fields `run(_, _, i)` touches, as `&` (reads) or `&mut` (writes), and
/// `run` touches no other part of the state.
pub unsafe trait State: Send + Sync + Sized + 'static {
    /// What a failing step returns. `Infallible` when no step can fail.
    type Error: Send + 'static;
    /// The type of the host's pool its steps read; `()` when they read none.
    type Pool: Send + Sync + 'static;
    /// The ordinary steps, then the stop step when `HAS_STOP`.
    const STEPS: &'static [StepInfo];
    const HAS_STOP: bool;
    /// Whether a frame of a type step i receives is available to this worker.
    fn frame_offered(i: usize) -> bool;
    /// The message types step i receives.
    fn tags(i: usize) -> Tags;
    /// Run step i.
    ///
    /// # Safety
    /// `this` points to a live state, and the caller holds step i's fields: no other code reads a
    /// field of its write set or writes a field of its read or write set until this returns.
    unsafe fn run(this: *mut Self, pool: &Self::Pool, i: usize) -> Result<Status, Self::Error>;
}

/// The state of one worker and, once a step failed, its error. Shared by the leader's `Worker<S>`
/// handle and the backend.
pub(crate) struct Core<S: State> {
    state: UnsafeCell<ManuallyDrop<S>>,
    taken: AtomicBool,
    error: Mutex<Option<S::Error>>,
}

// SAFETY: steps reach the state only through `State::run`, under the field locks of the contract
// (backend.rs, S1), and the leader only through `edit` and `take`, when no step runs. `S: Sync` makes
// two steps reading one field sound; `S: Send` lets the state move to an agent's thread.
unsafe impl<S: State> Sync for Core<S> {}
// SAFETY: as above; the error crosses threads once, to the leader.
unsafe impl<S: State> Send for Core<S> {}

impl<S: State> Core<S> {
    pub(crate) fn new(state: S) -> Self {
        Core { state: UnsafeCell::new(ManuallyDrop::new(state)), taken: AtomicBool::new(false), error: Mutex::new(None) }
    }

    fn ptr(&self) -> *mut S {
        assert!(!self.taken.load(Ordering::Acquire), "trame: the state was already taken");
        // ManuallyDrop<S> is repr(transparent) over S.
        self.state.get().cast::<S>()
    }

    /// The leader's edit: `f` gets the whole state.
    ///
    /// # Safety
    /// No step of this worker runs or starts until it returns (backend.rs, S4).
    pub(crate) unsafe fn edit_with<R>(&self, f: impl FnOnce(&mut S) -> R) -> R {
        // SAFETY: exclusive by the caller's promise.
        f(unsafe { &mut *self.ptr() })
    }

    /// Move the state out.
    ///
    /// # Safety
    /// The worker has a result, so no step runs again.
    pub(crate) unsafe fn take(&self) -> S {
        let p = self.ptr();
        self.taken.store(true, Ordering::Release);
        // SAFETY: `taken` keeps it from being read or dropped again.
        unsafe { core::ptr::read(p) }
    }

    pub(crate) fn take_error(&self) -> Option<S::Error> {
        self.error.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl<S: State> Drop for Core<S> {
    fn drop(&mut self) {
        if !*self.taken.get_mut() {
            // SAFETY: not taken, and `&mut self` is the only access.
            unsafe { ManuallyDrop::drop(self.state.get_mut()) }
        }
    }
}

/// A worker as a backend sees it.
pub(crate) trait Erased: Send + Sync {
    fn steps(&self) -> &'static [StepInfo];
    fn has_stop(&self) -> bool;
    fn pool_type(&self) -> TypeId;
    fn pool_name(&self) -> &'static str;
    fn frame_offered(&self, i: usize) -> bool;
    fn tags(&self, i: usize) -> Tags;
    /// Run step i. A failure is kept here for the leader, and answered as `Err(())`.
    ///
    /// # Safety
    /// The caller holds step i's fields (backend.rs, S1). `pool` is the host's pool, of `pool_type`,
    /// or `None` when that is `()`.
    unsafe fn run(&self, i: usize, pool: Option<&(dyn Any + Send + Sync)>) -> Result<Status, ()>;
}

impl<S: State> Erased for Core<S> {
    fn steps(&self) -> &'static [StepInfo] {
        S::STEPS
    }
    fn has_stop(&self) -> bool {
        S::HAS_STOP
    }
    fn pool_type(&self) -> TypeId {
        TypeId::of::<S::Pool>()
    }
    fn pool_name(&self) -> &'static str {
        core::any::type_name::<S::Pool>()
    }
    fn frame_offered(&self, i: usize) -> bool {
        S::frame_offered(i)
    }
    fn tags(&self, i: usize) -> Tags {
        S::tags(i)
    }
    unsafe fn run(&self, i: usize, pool: Option<&(dyn Any + Send + Sync)>) -> Result<Status, ()> {
        static UNIT: () = ();
        let pool: &S::Pool = match pool {
            Some(p) => p.downcast_ref::<S::Pool>().expect("trame: the launch checked the pool's type"),
            None => (&UNIT as &dyn Any).downcast_ref::<S::Pool>().expect("trame: a worker without a pool has Pool = ()"),
        };
        // SAFETY: the caller holds step i's fields.
        match unsafe { S::run(self.ptr(), pool, i) } {
            Ok(s) => Ok(s),
            Err(e) => {
                *self.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
                Err(())
            }
        }
    }
}

/// The initial model of a worker: its steps' sets and weights.
pub(crate) fn model(w: &dyn Erased) -> Sched {
    let steps: Vec<(u64, u64, Weight)> = w.steps().iter().map(|s| (s.reads, s.writes, s.weight)).collect();
    Sched::new(&steps, w.has_stop())
}

/// What the reference scheduler keeps per worker, under one lock.
pub(crate) struct Inner {
    pub s: Sched,
    /// The conditions of each waiting step.
    pub waits: [Waits; crate::sched::MAX_STEPS],
    /// A leader edit waits for running steps: no step starts meanwhile.
    pub editing: bool,
    /// The trace, when the launch asked for one (`Topology::__trace`).
    pub trace: Option<Vec<Event>>,
    /// A step unwound mid-way (the run aborted): its fields are never given back.
    pub dead: bool,
}

/// One worker under the reference scheduler: shared by its agents and the leader.
pub(crate) struct Slot {
    pub obj: Arc<dyn Erased>,
    pub inner: Mutex<Inner>,
    /// Signalled at every End, edit and stop: an edit waits on it for running steps.
    pub changed: Condvar,
}

impl Slot {
    pub(crate) fn new(obj: Arc<dyn Erased>, trace: bool) -> Slot {
        let s = model(&*obj);
        let inner = Inner { s, waits: [Waits::default(); crate::sched::MAX_STEPS], editing: false, trace: trace.then(Vec::new), dead: false };
        Slot { obj, inner: Mutex::new(inner), changed: Condvar::new() }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(inner: &mut Inner, e: Event) {
        if let Some(t) = &mut inner.trace {
            t.push(e);
        }
    }

    /// Stop: from now on only the stop step may start (sched.rs). The backend then wakes the agents.
    pub(crate) fn stop(&self) {
        let mut g = self.lock();
        if !g.s.stopping {
            g.s.stopping = true;
            Slot::record(&mut g, Event::Stop);
        }
        self.changed.notify_all();
    }

    /// Edit: wait until no step runs, keep any from starting, run `f`, and wake every step. The
    /// backend then wakes the agents. A worker with a result is not edited: `f` is not run.
    pub(crate) fn edit(&self, f: &mut dyn FnMut()) -> bool {
        let mut g = self.lock();
        g.editing = true;
        while g.s.running() && !g.dead {
            g = self.changed.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
        let live = !g.dead && g.s.answer().is_none();
        if live {
            f();
            g.s.edited();
            Slot::record(&mut g, Event::Edit);
        }
        g.editing = false;
        self.changed.notify_all();
        live
    }

    pub(crate) fn trace(&self) -> Option<(Sched, Vec<Event>)> {
        let g = self.lock();
        g.trace.clone().map(|t| (model(&*self.obj), t))
    }
}

/// Whether some outgoing route in `mask` (bit k is route k, see `Backend::worker_route`) has room.
fn room_holds(mask: u64) -> bool {
    (0..64).any(|k| mask >> k & 1 == 1 && Selected::worker_room(k))
}

/// The conditions that hold now, for the waiting steps in `ps` (bit i per step).
fn conditions(obj: &dyn Erased, s: &Sched, waits: &[Waits]) -> Now {
    let mut out = Now::default();
    for (i, w) in waits.iter().enumerate() {
        if !matches!(s.ps[i].st, St::Wait { .. }) {
            continue;
        }
        if w.frame_wanted() && obj.frame_offered(i) {
            out.frame |= 1 << i;
        }
        if w.room_mask() != 0 && room_holds(w.room_mask()) {
            out.room |= 1 << i;
        }
        if let Some(at) = w.deadline()
            && time::now().at() >= at.at()
        {
            out.until |= 1 << i;
        }
    }
    out
}

/// The earliest deadline of a step that waits on the clock, in nanoseconds.
fn next_deadline(s: &Sched, waits: &[Waits]) -> Option<u64> {
    waits
        .iter()
        .enumerate()
        .filter(|(i, _)| matches!(s.ps[*i].st, St::Wait { until: true, .. }))
        .filter_map(|(_, w)| w.deadline())
        .map(|at| u64::try_from(at.at().as_nanos()).unwrap_or(u64::MAX))
        .min()
}

/// The status a step returned, as the model's End takes it. `service` is the backend's measure.
fn ended(r: Result<Status, ()>, clock: time::ClockId, service: u64) -> (Ended, Waits) {
    let (st, waits, weight) = match r {
        Err(()) => (St::Err, Waits::default(), None),
        Ok(s) => match s.kind() {
            Kind::Progress => (St::Progress, Waits::default(), s.new_weight()),
            Kind::Done => (St::Done, Waits::default(), None),
            Kind::Wait(w) => {
                if let Some(at) = w.deadline() {
                    assert!(
                        at.clock() == clock,
                        "trame: .until was given a reading of another clock; a worker waits only on its own clock"
                    );
                }
                let st = St::Wait { frame: w.frame_wanted(), room: w.room_mask() != 0, until: w.deadline().is_some() };
                (st, w, s.new_weight())
            }
        },
    };
    (Ended { st, weight, service }, waits)
}

/// One agent of the reference scheduler: run steps of `slot` until the worker has a result. Several
/// agents may run one slot at once; each step runs on the agent that started it. `service` measures a
/// step that ran from `t0` (seq: 1 per step).
pub(crate) fn agent(slot: &Slot, service: &dyn Fn() -> u64, pool: &dyn Fn() -> Option<Arc<dyn Any + Send + Sync>>) -> Answer {
    let clock = time::now().clock();
    let obj = &*slot.obj;
    loop {
        // Read before the conditions: a change after this read ends the idle below.
        let epoch = Selected::worker_epoch();
        let (snapshot, waits) = {
            let g = slot.lock();
            (g.s, g.waits)
        };
        // The conditions are read without the lock: they call the backend, which may yield.
        let now = conditions(obj, &snapshot, &waits[..snapshot.ps.len().min(waits.len())]);
        let mut g = slot.lock();
        if let Some(a) = g.s.answer() {
            slot.changed.notify_all();
            return a;
        }
        let pick = if g.editing { None } else { g.s.choose(&now) };
        match pick {
            Some(i) => {
                g.s.start(i);
                Slot::record(&mut g, Event::Start { i, now });
                drop(g);
                let pin = pool();
                let start = service();
                let _unwinding = Unwinding(slot);
                let r = {
                    let _owning = Owning::new(obj.tags(i));
                    // SAFETY: Start took step i's fields in the model, and every start goes through it.
                    unsafe { obj.run(i, pin.as_deref()) }
                };
                core::mem::forget(_unwinding);
                let (e, w) = ended(r, clock, service().saturating_sub(start).max(1));
                drop(pin);
                let mut g = slot.lock();
                g.s.end(i, e);
                g.waits[i] = w;
                Slot::record(&mut g, Event::End { i, e });
                slot.changed.notify_all();
            }
            // Another agent runs a step: its End may enable one. Wait for it, not for the backend.
            None if g.s.running() => {
                drop(slot.changed.wait(g).unwrap_or_else(PoisonError::into_inner));
            }
            None => {
                let deadline = next_deadline(&g.s, &g.waits);
                drop(g);
                Selected::worker_idle(deadline, epoch);
            }
        }
    }
}

/// Marks the slot dead if dropped, which happens only when a step unwinds: a step that returns
/// forgets it.
struct Unwinding<'a>(&'a Slot);

impl Drop for Unwinding<'_> {
    fn drop(&mut self) {
        self.0.lock().dead = true;
        self.0.changed.notify_all();
    }
}
