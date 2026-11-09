//! A worker is a state and its steps (INTERFACE.md 5.3). This file has three parts:
//!
//! 1. `State`, which `#[trame::state]` implements: the steps' read and write sets, and a `run` that
//!    calls step i with its fields. The state lives in `Core`, behind a raw pointer: steps that hold
//!    disjoint fields run at once, so no `&mut S` to the whole state may exist while they run.
//! 2. `Erased`: the worker as a backend sees it, without its type.
//! 3. `Slot` and `agent`: the reference scheduler, the model of sched.rs driven by real calls. A
//!    backend may use it (one that runs steps one at a time does) or schedule by itself; either way
//!    it keeps the contract of backend.rs, and conformance replays its trace against sched.rs.

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
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
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
    /// What a failing step returns. `Infallible` when no step can fail. Plain, because the leader
    /// reads it from the worker's domain, which for a worker in another process is that process's
    /// (CONTRACT.md M2).
    type Error: crate::Plain;
    /// The type of the host's pool its steps read; `()` when they read none.
    type Pool: crate::Plain;
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

/// What a worker's domain holds for it (CONTRACT.md, M1): its state and, once a step failed, its
/// error. The leader builds it there at launch; a worker in another process adopts its address.
struct Block<S: State> {
    state: ManuallyDrop<S>,
    /// Set by the first step that fails; it alone writes `error`.
    failed: AtomicBool,
    error: UnsafeCell<Option<S::Error>>,
}

/// One worker's state, as the leader's `Worker<S>` handle and the backend share it: the address of
/// its block, null until the builder ran (or the worker process adopted it).
pub(crate) struct Core<S: State> {
    block: AtomicPtr<Block<S>>,
    taken: AtomicBool,
    /// Debug builds: the fields held by running steps, kept here apart from any scheduler, so S1 is
    /// checked against what actually runs (backend.rs). Written fields, and readers per field.
    #[cfg(debug_assertions)]
    witness: Mutex<(u64, [u32; 64])>,
}

// SAFETY: steps reach the state only through `State::run`, under the field locks of the contract
// (CONTRACT.md, S1), and the leader only through `edit` and `take`, when no step runs. `S: Sync` makes
// two steps reading one field sound; `S: Send` lets the state move to an agent's thread. The error is
// written once, by the step that set `failed`, and read by the leader after the run.
unsafe impl<S: State> Sync for Core<S> {}
// SAFETY: as above.
unsafe impl<S: State> Send for Core<S> {}

impl<S: State> Core<S> {
    /// A worker whose state is not built yet.
    pub(crate) fn empty() -> Self {
        Core {
            block: AtomicPtr::new(core::ptr::null_mut()),
            taken: AtomicBool::new(false),
            #[cfg(debug_assertions)]
            witness: Mutex::new((0, [0; 64])),
        }
    }

    /// The block. Never turned into a `&Block<S>`: steps hold `&mut` to fields of the state while
    /// other agents use the block, so every access projects a field from this raw pointer.
    fn raw(&self) -> *mut Block<S> {
        let b = self.block.load(Ordering::Acquire);
        assert!(
            !b.is_null(),
            "trame: this worker's state is not in this process: never built, or built on another machine, whose leader is not built yet"
        );
        b
    }

    fn ptr(&self) -> *mut S {
        assert!(!self.taken.load(Ordering::Acquire), "trame: the state was already taken");
        // SAFETY: a live block (`raw`); a field projection creates no reference. ManuallyDrop<S> is
        // repr(transparent) over S.
        unsafe { (&raw mut (*self.raw()).state).cast::<S>() }
    }

    /// The block's `failed` flag and error cell, as references to those fields alone.
    fn failure(&self) -> (&AtomicBool, &UnsafeCell<Option<S::Error>>) {
        let b = self.raw();
        // SAFETY: a live block; references to two fields the state does not contain, shared only
        // through atomics and the write-once rule of `fail`.
        unsafe { (&*(&raw const (*b).failed), &*(&raw const (*b).error)) }
    }

    /// Put the built state in its block. Once, before any step; the caller is in arena mode
    /// (alloc.rs), so the block lands in the domain with everything the state owns.
    pub(crate) fn fill(&self, s: S) {
        let b = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(Block {
            state: ManuallyDrop::new(s),
            failed: AtomicBool::new(false),
            error: UnsafeCell::new(None),
        }));
        let was = self.block.swap(b, Ordering::AcqRel);
        assert!(was.is_null(), "trame: a state was built twice");
    }

    /// The address of the block, for a worker in another process to adopt (`adopt`).
    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    pub(crate) fn addr(&self) -> usize {
        self.block.load(Ordering::Acquire) as usize
    }

    /// Use the block another process built at `addr`, in a domain this process maps at the same
    /// address (CONTRACT.md, M1). This process never frees it.
    ///
    /// # Safety
    /// `addr` is the `addr()` of a `Core<S>` of the same type, filled, in a mapped domain.
    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    pub(crate) unsafe fn adopt(&self, addr: usize) {
        let was = self.block.swap(addr as *mut Block<S>, Ordering::AcqRel);
        assert!(was.is_null(), "trame: a worker adopted a state it already had");
        self.taken.store(false, Ordering::Release);
    }

    /// Keep the first failure's error; a later one is dropped.
    fn fail(&self, e: S::Error) {
        let (failed, error) = self.failure();
        if !failed.swap(true, Ordering::AcqRel) {
            // SAFETY: only the step that set `failed` writes, and the leader reads after the run.
            unsafe { *error.get() = Some(e) };
        }
    }

    /// The leader's edit: `f` gets the whole state.
    ///
    /// # Safety
    /// No step of this worker runs or starts until it returns (CONTRACT.md, S4).
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

    /// The error of the failed step, once. The worker has a result.
    pub(crate) fn take_error(&self) -> Option<S::Error> {
        let (failed, error) = self.failure();
        // SAFETY: the run is over, so no step writes it.
        failed.load(Ordering::Acquire).then(|| unsafe { (*error.get()).take() }).flatten()
    }
}

#[cfg(debug_assertions)]
impl<S: State> Core<S> {
    /// Take (`on`) or give back step i's fields in the witness; panics when S1 does not hold.
    fn hold(&self, i: usize, on: bool) {
        let info = S::STEPS[i];
        let reads = info.reads & !info.writes;
        let mut w = self.witness.lock().unwrap_or_else(PoisonError::into_inner);
        if on {
            let read_held = (0..64).filter(|&k| w.1[k] > 0).fold(0u64, |m, k| m | 1 << k);
            assert!(
                (info.reads | info.writes) & w.0 == 0 && info.writes & read_held == 0,
                "trame: S1 violated: step `{}` started while a field it uses was held (written {:#x}, read {:#x})",
                info.name,
                w.0,
                read_held
            );
            w.0 |= info.writes;
        } else {
            w.0 &= !info.writes;
        }
        for k in 0..64 {
            if reads >> k & 1 == 1 {
                if on { w.1[k] += 1 } else { w.1[k] -= 1 }
            }
        }
    }
}

/// The pool: read-only shared data. Steps that take `pool: &P` read it under the `POOL` bit
/// (sched.rs); only the leader writes it, in an edit holding that bit in every worker (CONTRACT.md, S4).
pub(crate) struct PoolCore<P> {
    /// The pool's block in the domain (CONTRACT.md, M1); null until built or adopted.
    value: AtomicPtr<P>,
}

// SAFETY: steps share `&P` (P: Sync) and the leader writes only while it holds the POOL bit everywhere.
unsafe impl<P: Send + Sync> Sync for PoolCore<P> {}
// SAFETY: as above.
unsafe impl<P: Send + Sync> Send for PoolCore<P> {}

impl<P> PoolCore<P> {
    pub(crate) fn empty() -> Self {
        PoolCore { value: AtomicPtr::new(core::ptr::null_mut()) }
    }

    /// Put the built pool in its block. Once, at launch, before any step, in arena mode.
    pub(crate) fn fill(&self, p: P) {
        let b = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(p));
        let was = self.value.swap(b, Ordering::AcqRel);
        assert!(was.is_null(), "trame: the pool was built twice");
    }

    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    pub(crate) fn addr(&self) -> usize {
        self.value.load(Ordering::Acquire) as usize
    }

    /// As `Core::adopt`.
    ///
    /// # Safety
    /// `addr` is the `addr()` of a filled `PoolCore<P>`, in a mapped domain.
    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    pub(crate) unsafe fn adopt(&self, addr: usize) {
        let was = self.value.swap(addr as *mut P, Ordering::AcqRel);
        assert!(was.is_null(), "trame: a worker adopted a pool it already had");
    }

    fn at(&self) -> *mut P {
        let p = self.value.load(Ordering::Acquire);
        assert!(!p.is_null(), "trame: the pool was never built");
        p
    }

    /// # Safety
    /// The caller holds the POOL bit for reading (a step that takes the pool).
    pub(crate) unsafe fn get(&self) -> &P {
        // SAFETY: built, and no writer while the caller reads.
        unsafe { &*self.at() }
    }

    /// # Safety
    /// The caller holds the POOL bit in every worker for writing (CONTRACT.md, S4).
    pub(crate) unsafe fn edit_with<R>(&self, f: impl FnOnce(&mut P) -> R) -> R {
        // SAFETY: built; the caller excludes every reader.
        f(unsafe { &mut *self.at() })
    }
}

/// The pool as a backend sees it, without its type.
pub(crate) trait ErasedPool: Send + Sync {
    /// For a step's downcast to `PoolCore<P>` (`Erased::run`).
    fn any(&self) -> &(dyn Any + Send + Sync);
    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    fn addr(&self) -> usize;
    /// # Safety
    /// As `PoolCore::adopt`.
    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    unsafe fn adopt(&self, addr: usize);
}

impl<P: Send + Sync + 'static> ErasedPool for PoolCore<P> {
    fn any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
    fn addr(&self) -> usize {
        PoolCore::addr(self)
    }
    unsafe fn adopt(&self, addr: usize) {
        // SAFETY: the caller's.
        unsafe { PoolCore::adopt(self, addr) }
    }
}

impl<P> Drop for PoolCore<P> {
    fn drop(&mut self) {
        let p = *self.value.get_mut();
        if !p.is_null() {
            // SAFETY: built by `fill` in this process (an adopting worker process never drops).
            drop(unsafe { alloc::boxed::Box::from_raw(p) });
        }
    }
}

impl<S: State> Drop for Core<S> {
    fn drop(&mut self) {
        let b = *self.block.get_mut();
        if b.is_null() {
            return;
        }
        // SAFETY: built by `fill` in this process (a worker process that adopted never drops: it
        // ends with process::exit), and `&mut self` is the only access.
        let mut b = unsafe { alloc::boxed::Box::from_raw(b) };
        if !*self.taken.get_mut() {
            // SAFETY: not taken, so the state is still there.
            unsafe { ManuallyDrop::drop(&mut b.state) }
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
    /// The state block's address (`Core::addr`), and its adoption by a worker in another process.
    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    fn addr(&self) -> usize;
    /// # Safety
    /// As `Core::adopt`.
    #[allow(dead_code)] // used by a backend whose workers adopt a state built in another process
    unsafe fn adopt(&self, addr: usize);
    fn tags(&self, i: usize) -> Tags;
    /// Run step i. A failure is kept here for the leader, and answered as `Err(())`.
    ///
    /// # Safety
    /// The caller holds step i's fields (CONTRACT.md, S1). `pool` is the launch's pool, a
    /// `PoolCore<pool_type>`, or `None` when that is `()`.
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
    fn addr(&self) -> usize {
        Core::addr(self)
    }
    unsafe fn adopt(&self, addr: usize) {
        // SAFETY: the caller's.
        unsafe { Core::adopt(self, addr) }
    }
    fn tags(&self, i: usize) -> Tags {
        S::tags(i)
    }
    unsafe fn run(&self, i: usize, pool: Option<&(dyn Any + Send + Sync)>) -> Result<Status, ()> {
        static UNIT: () = ();
        let pool: &S::Pool = match pool {
            // SAFETY: a step that takes the pool reads the POOL bit, held with its fields.
            Some(p) => unsafe { p.downcast_ref::<PoolCore<S::Pool>>().expect("trame: the launch checked the pool's type").get() },
            None => (&UNIT as &dyn Any).downcast_ref::<S::Pool>().expect("trame: a worker without a pool has Pool = ()"),
        };
        #[cfg(debug_assertions)]
        self.hold(i, true);
        // SAFETY: the caller holds step i's fields.
        let r = unsafe { S::run(self.ptr(), pool, i) };
        #[cfg(debug_assertions)]
        self.hold(i, false);
        match r {
            Ok(s) => Ok(s),
            Err(e) => {
                self.fail(e);
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
    /// The trace, when the launch asked for one (`Topology::__trace`).
    pub trace: Option<Vec<Event>>,
    /// The fields a leader in another process asked this worker to hold (`remote_edit`), 0 if none;
    /// and whether this worker told it they are held.
    pub remote: u64,
    pub acked: bool,
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
        let inner = Inner { s, waits: [Waits::default(); crate::sched::MAX_STEPS], trace: trace.then(Vec::new), dead: false, remote: 0, acked: false };
        Slot { obj, inner: Mutex::new(inner), changed: Condvar::new() }
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn record(inner: &mut Inner, e: Event) {
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

    pub(crate) fn trace(&self) -> Option<(Sched, Vec<Event>)> {
        let g = self.lock();
        g.trace.clone().map(|t| (model(&*self.obj), t))
    }
}

/// A leader in another process edits this worker (backend.rs, `worker_edit_request`): `req` is the
/// fields it asks for, 0 when it asks none. Hold them once; tell it once no step touching them runs;
/// once it withdraws the request (it wrote them), give them back, waking the steps that touch them.
fn remote_edit(slot: &Slot, g: &mut Inner, req: u64) {
    if req != 0 && g.remote == 0 {
        g.s.editing |= req;
        g.remote = req;
        g.acked = false;
        Slot::record(g, Event::Hold { fields: req });
    }
    if g.remote != 0 && !g.acked && !g.s.touching(g.remote) {
        Selected::worker_edit_held(true);
        g.acked = true;
    }
    if g.remote != 0 && g.acked && req == 0 {
        let fields = g.remote;
        g.s.edited(fields);
        Slot::record(g, Event::Edit { fields });
        g.remote = 0;
        g.acked = false;
        Selected::worker_edit_held(false);
        slot.changed.notify_all();
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
pub(crate) fn agent(slot: &Slot, service: &dyn Fn() -> u64, pool: Option<&(dyn Any + Send + Sync)>) -> Answer {
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
        if Selected::worker_stop_requested() {
            slot.stop();
        }
        let mut g = slot.lock();
        if g.dead {
            drop(g);
            Selected::worker_abandon();
        }
        if let Some(a) = g.s.answer() {
            slot.changed.notify_all();
            return a;
        }
        remote_edit(slot, &mut g, Selected::worker_edit_request());
        let pick = g.s.choose(&now);
        match pick {
            Some(i) => {
                g.s.start(i);
                Selected::worker_mark_start();
                Slot::record(&mut g, Event::Start { i, now });
                drop(g);
                let start = service();
                let _unwinding = Unwinding(slot);
                let r = {
                    let _owning = Owning::new(obj.tags(i));
                    // SAFETY: Start took step i's fields in the model, and every start goes through it.
                    unsafe { obj.run(i, pool) }
                };
                core::mem::forget(_unwinding);
                let (e, w) = ended(r, clock, service().saturating_sub(start).max(1));
                let mut g = slot.lock();
                g.s.end(i, e);
                Selected::worker_mark_end();
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
                Selected::worker_mark_quiet(epoch, deadline.is_some());
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
