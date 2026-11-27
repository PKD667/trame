//! The reference model of one worker's scheduler: field locks and weighted stride. Plain data and pure
//! functions; no clock, thread or I/O. Every backend schedules steps; this file says what a schedule
//! may be, and `check` (below) replays a backend's trace against it.
//!
//! A worker has steps 0..len and, optionally, a stop step at index `len`. Step i reads the fields
//! R(i) and writes the fields W(i), as bitmasks over the state's fields (at most 64).
//!
//! Safety, which every backend keeps exactly and `check` verifies on every trace:
//!   - Start(i) needs i enabled (Fresh, Progress, or a Wait that was woken or whose condition holds),
//!     i not running, and its fields free: no field of R(i) ∪ W(i) written by a running step, and no
//!     field of W(i) read by a running step. It takes them all at once; End(i) gives them back.
//!   - End(i) records i's status and wakes every j ≠ i with W(i) ∩ (R(j) ∪ W(j)) ≠ ∅.
//!   - After a step fails, no step starts. After a stop, no ordinary step starts, and the stop step
//!     starts only when no step is running.
//!
//! Choice, which is the fairness model. Each step has a weight w in [0, 1] and a pass. Among the steps
//! that may start, the least pass among positive weights wins (ties: the lower index); a weight-0 step
//! starts only when no positive-weight step may, and then the least recently started (ties: the lower
//! index). End(i) adds service(i) · 2^24/w to i's pass, where service is the backend's measure of what
//! the step used (1 per step on a backend that runs one step at a time, nanoseconds on a threaded backend). So, under contention, the
//! service of positive-weight steps is shared in proportion to their weights (stride scheduling,
//! Waldspurger and Weihl 1995). A step that was not ready does not bank its absence: when a step
//! starts, every waiting step's pass is raised to at least the started step's pass.
//!
//! A backend with `Backend::EXACT` follows `choose` exactly, and `check` can verify that. A backend with several agents may start
//! any step `may_start` allows; its weights are checked by measured shares (conformance), not by
//! replaying choices.

/// The most steps of one worker, the stop step included: masks are 64 bits.
pub const MAX_STEPS: usize = 64;

/// The pool's bit in a step's read set: a step that takes `pool: &P` reads it. A state's own fields
/// are bits 0..63, so a state has at most 63 fields.
pub const POOL: u64 = 1 << 63;
/// A whole-state edit (`Run::edit` of a worker): every field but the pool.
pub const STATE: u64 = !POOL;

/// A weight in [0, 1], fixed point: 0 is 0, `Weight::MAX` is 1. Fixed point keeps an exact backend deterministic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Weight(pub u16);

impl Weight {
    pub const ZERO: Weight = Weight(0);
    pub const MAX: Weight = Weight(u16::MAX);

    /// The weight nearest to `w`. Panics outside [0, 1], or on NaN.
    /// Const, so a step's static weight is checked at compile time.
    pub const fn from_f64(w: f64) -> Weight {
        assert!(w >= 0.0 && w <= 1.0, "trame: a weight is in [0, 1]");
        Weight((w * 65535.0 + 0.5) as u16)
    }

    pub fn to_f64(self) -> f64 {
        f64::from(self.0) / f64::from(u16::MAX)
    }

    /// The pass a step of this weight gains per unit of service; 0 for weight 0 (not counted).
    pub fn stride(self) -> u64 {
        if self.0 == 0 { 0 } else { (u64::from(u16::MAX) << 24) / u64::from(self.0) }
    }
}

/// St(i): what step i last returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum St {
    Fresh,
    Progress,
    Done,
    /// A wait, with which conditions it named. `fields` is the bare wait: no condition, woken only by
    /// a write to one of its fields.
    Wait {
        frame: bool,
        room: bool,
    },
    Err,
}

/// One step, as the scheduler sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    pub reads: u64,
    pub writes: u64,
    pub st: St,
    /// D(i): a step that writes one of i's fields ended since i last ended.
    pub d: bool,
    pub running: bool,
    pub weight: Weight,
    pub pass: u64,
    /// The start count when i last started (0: never), for the weight-0 round robin.
    pub last: u64,
}

impl Proc {
    pub const fn new(reads: u64, writes: u64, weight: Weight) -> Proc {
        Proc { reads, writes, st: St::Fresh, d: false, running: false, weight, pass: 0, last: 0 }
    }

    fn fields(&self) -> u64 {
        self.reads | self.writes
    }
}

/// Which wait conditions hold now, one bit per step (bit i). Computed by the backend; bit i is only
/// read for a step whose wait names that condition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Now {
    pub frame: u64,
    pub room: u64,
}

/// The fields held by running steps: written ones, and a reader count per field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locks {
    pub written: u64,
    pub readers: [u16; 64],
}

impl Default for Locks {
    fn default() -> Self {
        Locks { written: 0, readers: [0; 64] }
    }
}

impl Locks {
    pub fn read(&self) -> u64 {
        (0..64).filter(|&k| self.readers[k] > 0).fold(0, |m, k| m | 1 << k)
    }

    /// Whether a step with these sets may take its fields now.
    pub fn free(&self, reads: u64, writes: u64) -> bool {
        (reads | writes) & self.written == 0 && writes & self.read() == 0
    }

    fn take(&mut self, p: &Proc) {
        debug_assert!(self.free(p.reads, p.writes));
        self.written |= p.writes;
        for k in 0..64 {
            // A field both read and written is held as written only.
            if (p.reads & !p.writes) >> k & 1 == 1 {
                self.readers[k] += 1;
            }
        }
    }

    fn give(&mut self, p: &Proc) {
        self.written &= !p.writes;
        for k in 0..64 {
            if (p.reads & !p.writes) >> k & 1 == 1 {
                self.readers[k] -= 1;
            }
        }
    }
}

/// One worker's scheduler state. `ps[..len]` are the ordinary steps; `ps[len]`, when `has_stop`, is
/// the stop step.
#[derive(Clone, Copy, Debug)]
pub struct Sched {
    pub ps: [Proc; MAX_STEPS],
    /// Fields the leader holds for an edit (`Event::Hold`): no step touching one starts until the
    /// edit (`Event::Edit`) gives them back.
    pub editing: u64,
    pub len: usize,
    pub has_stop: bool,
    pub locks: Locks,
    pub stopping: bool,
    /// Starts so far.
    pub starts: u64,
}

/// A worker's result, once it has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Every ordinary step is done.
    Returned,
    /// Stopped: the stop step is done, or there is none, and nothing runs.
    Stopped,
    /// Step i failed and nothing runs.
    Failed(usize),
}

/// What a step returned, as End takes it: its status, the weight for its next start (None: keep),
/// and the service it used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ended {
    pub st: St,
    pub weight: Option<Weight>,
    pub service: u64,
}

impl Sched {
    /// `steps[i]` = (R, W, initial weight); the stop step, when there is one, is last.
    pub fn new(steps: &[(u64, u64, Weight)], has_stop: bool) -> Sched {
        let total = steps.len();
        assert!(total <= MAX_STEPS, "trame: a worker has at most {MAX_STEPS} steps, the stop step included");
        assert!(!has_stop || total >= 1, "trame: a stop step was declared but no step was given");
        let mut ps = [Proc::new(0, 0, Weight::ZERO); MAX_STEPS];
        for (p, &(r, w, wt)) in ps.iter_mut().zip(steps) {
            *p = Proc::new(r, w, wt);
        }
        Sched {
            ps,
            editing: 0,
            len: total - usize::from(has_stop),
            has_stop,
            locks: Locks::default(),
            stopping: false,
            starts: 0,
        }
    }

    fn total(&self) -> usize {
        self.len + usize::from(self.has_stop)
    }

    pub fn running(&self) -> bool {
        self.ps[..self.total()].iter().any(|p| p.running)
    }

    fn failed(&self) -> bool {
        self.ps[..self.total()].iter().any(|p| p.st == St::Err)
    }

    /// Whether step i's status lets it start now (not counting its fields).
    pub fn enabled(&self, i: usize, now: &Now) -> bool {
        let p = &self.ps[i];
        let bit = |m: u64| m >> i & 1 == 1;
        match p.st {
            St::Fresh | St::Progress => true,
            St::Done | St::Err => false,
            St::Wait { frame, room } => p.d || (frame && bit(now.frame)) || (room && bit(now.room)),
        }
    }

    /// Whether i wants to start: every premise of Start but the fields (enabled, not running, the
    /// stop and failure rules).
    pub fn wants(&self, i: usize, now: &Now) -> bool {
        if i >= self.total() || self.failed() || self.ps[i].running || !self.enabled(i, now) {
            return false;
        }
        let ordinary = i < self.len;
        if self.stopping && (ordinary || self.running()) {
            return false;
        }
        self.stopping || ordinary
    }

    /// Who gets a contended field first (CONTRACT.md, S6): positive weights before weight 0; among positive
    /// weights the lowest pass, among weight 0 the earliest last start; then the index.
    pub fn rank(&self, i: usize) -> (bool, u64, usize) {
        let p = &self.ps[i];
        (p.weight.0 == 0, if p.weight.0 > 0 { p.pass } else { p.last }, i)
    }

    /// Whether step a's fields conflict with the sets (r, w): one of them writes what the other uses.
    fn conflicts(a: &Proc, r: u64, w: u64) -> bool {
        (a.reads | a.writes) & w != 0 || a.writes & r != 0
    }

    /// The safety premise of Start: whether i may start now. Its fields are free (no running step
    /// and no edit holds them), and no step ranked above it that wants to start uses them: lock
    /// access goes by rank (CONTRACT.md, S6), so a step that waits for a field is not overtaken by a lower one
    /// that would hold it longer. The choice among several is `choose`.
    pub fn may_start(&self, i: usize, now: &Now) -> bool {
        if !self.wants(i, now) {
            return false;
        }
        let p = &self.ps[i];
        if (p.reads | p.writes) & self.editing != 0 || !self.locks.free(p.reads, p.writes) {
            return false;
        }
        let me = self.rank(i);
        !(0..self.total()).any(|j| {
            j != i
                && self.rank(j) < me
                && self.wants(j, now)
                && Sched::conflicts(p, self.ps[j].reads, self.ps[j].writes)
        })
    }

    /// Whether a running step touches one of `fields`: an edit of them waits until none does.
    pub fn touching(&self, fields: u64) -> bool {
        self.ps[..self.total()].iter().any(|p| p.running && (p.reads | p.writes) & fields != 0)
    }

    /// The model's choice: stride among positive weights, else round robin among weight 0.
    pub fn choose(&self, now: &Now) -> Option<usize> {
        // The startable step of the best rank. Equal to filtering by `may_start`, in one pass: walk
        // the steps that want to start in rank order, and keep the fields of those passed over.
        let mut order: [usize; MAX_STEPS] = [0; MAX_STEPS];
        let mut n = 0;
        for i in 0..self.total() {
            if self.wants(i, now) {
                order[n] = i;
                n += 1;
            }
        }
        order[..n].sort_unstable_by_key(|&i| self.rank(i));
        let (mut r, mut w) = (0u64, 0u64);
        for &i in &order[..n] {
            let p = &self.ps[i];
            let free = (p.reads | p.writes) & self.editing == 0 && self.locks.free(p.reads, p.writes);
            if free && !Sched::conflicts(p, r, w) {
                return Some(i);
            }
            r |= p.reads;
            w |= p.writes;
        }
        None
    }

    /// Start(i): the caller checked `may_start`.
    pub fn start(&mut self, i: usize) {
        self.starts += 1;
        let p = self.ps[i];
        self.locks.take(&p);
        let front = p.pass;
        let total = self.total();
        for q in self.ps[..total].iter_mut() {
            if matches!(q.st, St::Wait { .. }) && q.pass < front {
                q.pass = front;
            }
        }
        let p = &mut self.ps[i];
        p.running = true;
        p.last = self.starts;
    }

    /// End(i).
    pub fn end(&mut self, i: usize, e: Ended) {
        let p = self.ps[i];
        assert!(p.running, "trame: End of a step that is not running");
        self.locks.give(&p);
        let total = self.total();
        for (j, q) in self.ps[..total].iter_mut().enumerate() {
            if j != i && p.writes & q.fields() != 0 {
                q.d = true;
            }
        }
        let p = &mut self.ps[i];
        p.running = false;
        p.st = e.st;
        p.d = false;
        // The service was given under the running weight, so the pass is charged at it, and the step's new weight
        // applies from the next start (INTERFACE.md 5.3, End: the two updates are simultaneous there, and the pass
        // grows only when the running weight is above zero).
        p.pass = p.pass.saturating_add(e.service.saturating_mul(p.weight.stride()));
        if let Some(w) = e.weight {
            p.weight = w;
        }
    }

    /// A leader edit wrote every field: every step is woken (it is End of a step that writes all).
    /// The leader wrote `fields` (and holds them): every step touching one is woken, as by a write
    /// (S3), and the fields are given back.
    pub fn edited(&mut self, fields: u64) {
        assert!(!self.touching(fields), "trame: an edit runs only when no step touching its fields does");
        let total = self.total();
        for q in self.ps[..total].iter_mut() {
            if (q.reads | q.writes) & fields != 0 {
                q.d = true;
            }
        }
        self.editing &= !fields;
    }

    /// The worker's result, if it has one now.
    pub fn answer(&self) -> Option<Answer> {
        if self.running() {
            return None;
        }
        if let Some(i) = self.ps[..self.total()].iter().position(|p| p.st == St::Err) {
            return Some(Answer::Failed(i));
        }
        if self.stopping {
            let stop_done = !self.has_stop || self.ps[self.len].st == St::Done;
            return stop_done.then_some(Answer::Stopped);
        }
        self.ps[..self.len].iter().all(|p| p.st == St::Done).then_some(Answer::Returned)
    }
}

/// One event of a backend's trace of one worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Step i started; `now` is what the backend saw of the wait conditions.
    Start {
        i: usize,
        now: Now,
    },
    End {
        i: usize,
        e: Ended,
    },
    Stop,
    /// The leader holds `fields` for an edit: no step touching one starts.
    Hold {
        fields: u64,
    },
    /// The leader wrote `fields`, with no step touching one running, and gave them back.
    Edit {
        fields: u64,
    },
}

/// A violation of the model by a trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// Event n started a step that may not start.
    Start { n: usize, i: usize },
    /// Event n ended a step that is not running.
    End { n: usize, i: usize },
    /// Event n started step i where the model chooses another (only with `exact`).
    Choice { n: usize, i: usize, model: Option<usize> },
    /// Event n edited fields it did not hold, or while a step touching them runs.
    Edit { n: usize },
}

/// Replay a trace against the model. `exact`: also require the model's choice at each start (`Backend::EXACT`).
/// Answers the final state, for the result check.
pub fn check(mut s: Sched, trace: &[Event], exact: bool) -> Result<Sched, Violation> {
    for (n, ev) in trace.iter().enumerate() {
        match *ev {
            Event::Start { i, now } => {
                if !s.may_start(i, &now) {
                    return Err(Violation::Start { n, i });
                }
                if exact && s.choose(&now) != Some(i) {
                    return Err(Violation::Choice { n, i, model: s.choose(&now) });
                }
                s.start(i);
            }
            Event::End { i, e } => {
                if i >= s.total() || !s.ps[i].running {
                    return Err(Violation::End { n, i });
                }
                s.end(i, e);
            }
            Event::Stop => s.stopping = true,
            Event::Hold { fields } => s.editing |= fields,
            Event::Edit { fields } => {
                if s.touching(fields) || s.editing & fields != fields {
                    return Err(Violation::Edit { n });
                }
                s.edited(fields);
            }
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const NOW0: Now = Now { frame: 0, room: 0 };
    const W1: Weight = Weight::MAX;
    const PROGRESS: Ended = Ended { st: St::Progress, weight: None, service: 1 };

    fn ended(st: St) -> Ended {
        Ended { st, weight: None, service: 1 }
    }

    #[test]
    fn writers_of_one_field_exclude_each_other() {
        let mut s = Sched::new(&[(0, 1, W1), (0, 1, W1)], false);
        assert!(s.may_start(0, &NOW0), "step 0 ranks first");
        assert!(!s.may_start(1, &NOW0), "step 0 ranks above step 1 and writes its field (CONTRACT.md, S6)");
        s.start(0);
        assert!(!s.may_start(1, &NOW0), "field 0 is written by step 0");
        s.end(0, PROGRESS);
        assert!(s.may_start(1, &NOW0));
    }

    #[test]
    fn readers_share_and_exclude_a_writer() {
        let mut s = Sched::new(&[(1, 0, W1), (1, 0, W1), (0, 1, W1)], false);
        s.start(0);
        assert!(s.may_start(1, &NOW0), "two readers share field 0");
        s.start(1);
        assert!(!s.may_start(2, &NOW0), "a writer waits for readers");
        s.end(0, PROGRESS);
        assert!(!s.may_start(2, &NOW0), "one reader remains");
        s.end(1, PROGRESS);
        assert!(s.may_start(2, &NOW0));
        s.start(2);
        assert!(!s.may_start(0, &NOW0), "a reader waits for the writer");
    }

    #[test]
    fn disjoint_steps_run_together() {
        let mut s = Sched::new(&[(0b01, 0b10, W1), (0b100, 0b1000, W1)], false);
        s.start(0);
        assert!(s.may_start(1, &NOW0));
    }

    #[test]
    fn only_a_write_wakes() {
        // 0 waits bare on field 0 (reads it); 1 reads field 0; 2 writes field 0.
        let mut s = Sched::new(&[(1, 0, W1), (1, 0, W1), (0, 1, W1)], false);
        s.start(0);
        s.end(0, ended(St::Wait { frame: false, room: false }));
        s.start(1);
        s.end(1, PROGRESS);
        assert!(!s.enabled(0, &NOW0), "a reader's end does not wake");
        s.start(2);
        s.end(2, PROGRESS);
        assert!(s.enabled(0, &NOW0), "a writer's end wakes");
    }

    #[test]
    fn a_condition_wakes_only_the_step_that_named_it() {
        let mut s = Sched::new(&[(0, 1, W1), (0, 2, W1)], false);
        s.start(0);
        s.end(0, ended(St::Wait { frame: true, room: false }));
        s.start(1);
        s.end(1, ended(St::Wait { frame: false, room: true }));
        let both = Now { frame: 0b11, room: 0 };
        assert!(s.enabled(0, &both));
        assert!(!s.enabled(1, &both), "step 1 waits on room, not a frame");
    }

    #[test]
    fn nothing_starts_after_a_failure_and_the_answer_waits_for_running_steps() {
        let mut s = Sched::new(&[(0, 1, W1), (0, 2, W1), (0, 4, W1)], false);
        s.start(0);
        s.start(1);
        s.end(0, ended(St::Err));
        assert!(!s.may_start(2, &NOW0));
        assert_eq!(s.answer(), None, "step 1 still runs");
        s.end(1, PROGRESS);
        assert_eq!(s.answer(), Some(Answer::Failed(0)));
    }

    #[test]
    fn stop_runs_the_stop_step_alone() {
        let mut s = Sched::new(&[(0, 1, W1), (0, 2, W1), (1, 0, W1)], true);
        assert!(!s.may_start(2, &NOW0), "the stop step does not run before a stop");
        s.start(0);
        s.stopping = true;
        assert!(!s.may_start(1, &NOW0), "no ordinary step after a stop");
        assert!(!s.may_start(2, &NOW0), "the stop step waits for running steps");
        s.end(0, PROGRESS);
        assert_eq!(s.answer(), None);
        assert_eq!(s.choose(&NOW0), Some(2));
        s.start(2);
        s.end(2, ended(St::Done));
        assert_eq!(s.answer(), Some(Answer::Stopped));
    }

    #[test]
    fn stop_without_stop_step_answers_when_idle() {
        let mut s = Sched::new(&[(0, 1, W1)], false);
        s.stopping = true;
        assert_eq!(s.answer(), Some(Answer::Stopped));
    }

    /// Run the model alone, always choosing, for `n` starts; every step returns Progress with service 1.
    fn shares(steps: &[(u64, u64, Weight)], n: u64) -> Vec<u64> {
        let mut s = Sched::new(steps, false);
        let mut got = std::vec![0; steps.len()];
        for _ in 0..n {
            let i = s.choose(&NOW0).expect("a step is always ready");
            s.start(i);
            got[i] += 1;
            s.end(i, PROGRESS);
        }
        got
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "50 000 starts to resolve a share to 0.001: hours under Miri, and it reads no memory the other sched tests do not"
    )]
    fn contested_shares_follow_weights() {
        for (a, b) in [(1.0, 1.0), (1.0, 0.5), (1.0, 0.25), (0.5, 0.1), (0.2, 0.8)] {
            let got = shares(&[(0, 1, Weight::from_f64(a)), (0, 1, Weight::from_f64(b))], 10_000);
            let share = got[0] as f64 / 10_000.0;
            let want = a / (a + b);
            assert!((share - want).abs() < 0.001, "weights {a}, {b}: share {share}, model {want}");
        }
    }

    #[test]
    fn weight_zero_is_background_and_round_robin_among_itself() {
        let n = if cfg!(miri) { 60 } else { 1000 };
        let got = shares(&[(0, 1, Weight::from_f64(0.1)), (0, 1, Weight::ZERO)], n);
        assert_eq!(got, [n, 0], "a weight-0 step never beats a ready positive one");
        let got = shares(&[(0, 1, Weight::ZERO), (0, 1, Weight::ZERO), (0, 1, Weight::ZERO)], n - n % 3);
        assert_eq!(got, [n / 3, n / 3, n / 3]);
    }

    #[test]
    fn a_woken_step_does_not_bank_its_absence() {
        // Step 1 waits while step 0 runs 100 times, then is woken: it must not then run 100 times.
        let mut s = Sched::new(&[(0, 1, W1), (1, 2, W1)], false);
        s.start(1);
        s.end(1, ended(St::Wait { frame: true, room: false }));
        for _ in 0..100 {
            let i = s.choose(&NOW0).unwrap();
            s.start(i);
            s.end(i, PROGRESS);
        }
        let mut run = [0; 2];
        let all = Now { frame: 0b10, ..NOW0 };
        for _ in 0..20 {
            let i = s.choose(&all).unwrap();
            s.start(i);
            run[i] += 1;
            s.end(i, PROGRESS);
        }
        assert!(run[0] >= 9 && run[1] <= 11, "{run:?}");
    }

    #[test]
    fn check_accepts_the_models_own_trace_and_refuses_a_lock_violation() {
        let s0 = Sched::new(&[(0, 1, W1), (0, 1, W1)], false);
        let ok = [
            Event::Start { i: 0, now: NOW0 },
            Event::End { i: 0, e: PROGRESS },
            Event::Start { i: 1, now: NOW0 },
            Event::End { i: 1, e: PROGRESS },
        ];
        assert!(check(s0, &ok, true).is_ok());
        let bad = [Event::Start { i: 0, now: NOW0 }, Event::Start { i: 1, now: NOW0 }];
        assert_eq!(check(s0, &bad, false).unwrap_err(), Violation::Start { n: 1, i: 1 });
        // Step 1 overtaking step 0, which ranks above it and writes the same field: refused (CONTRACT.md, S6).
        let overtake = [Event::Start { i: 1, now: NOW0 }];
        assert_eq!(check(s0, &overtake, false).unwrap_err(), Violation::Start { n: 0, i: 1 });
        // On disjoint fields, another choice than the model's is safe, and only the exact replay refuses it.
        let s1 = Sched::new(&[(0, 1, W1), (0, 2, W1)], false);
        let other_choice = [Event::Start { i: 1, now: NOW0 }];
        assert!(check(s1, &other_choice, false).is_ok(), "a start that overtakes no conflicting step is safe");
        assert!(matches!(check(s1, &other_choice, true), Err(Violation::Choice { .. })));
    }

    /// Lock-access priority (S6): with two agents, two light readers of f0 that keep overlapping
    /// must not keep the heavy writer of f0 from ever starting. Each step costs 1; an agent that ends
    /// a step chooses again at once.
    #[test]
    fn a_heavy_writer_is_not_starved_by_overlapping_light_readers() {
        let f0 = 1u64;
        let light = Weight::from_f64(0.1);
        let mut s = Sched::new(&[(f0, 0, light), (f0, 0, light), (0, f0, Weight::from_f64(1.0))], false);
        let now = Now::default();
        let mut running: [Option<usize>; 2] = [None, None];
        let mut writer_starts = 0;
        for round in 0..(if cfg!(miri) { 60 } else { 1000 }) {
            let agent = round % 2;
            if let Some(i) = running[agent].take() {
                s.end(i, Ended { st: St::Progress, weight: None, service: 1 });
            }
            if let Some(i) = s.choose(&now) {
                s.start(i);
                running[agent] = Some(i);
                writer_starts += usize::from(i == 2);
            }
        }
        assert!(writer_starts > 0, "the writer of weight 1 never started in 1000 rounds");
    }

    /// `choose` is the best-ranked step that `may_start` admits, over random schedules: random read
    /// and write sets over 3 fields, weights (some 0), and runs of random starts and ends. Seeds
    /// 0..2000, xorshift; a failure names its seed.
    #[test]
    fn choose_is_the_best_ranked_step_may_start_admits() {
        for seed in 0..(if cfg!(miri) { 12 } else { 2000u64 }) {
            let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let mut next = move || {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x
            };
            let n = 2 + (next() % 5) as usize;
            let steps: std::vec::Vec<(u64, u64, Weight)> = (0..n)
                .map(|_| {
                    let w = next() % 8;
                    let r = next() % 8 & !w;
                    (r, w, Weight::from_f64((next() % 5) as f64 / 4.0))
                })
                .collect();
            let mut s = Sched::new(&steps, false);
            let now = Now::default();
            for _ in 0..40 {
                let model = (0..s.total()).filter(|&i| s.may_start(i, &now)).min_by_key(|&i| s.rank(i));
                assert_eq!(s.choose(&now), model, "seed {seed}");
                let running: std::vec::Vec<usize> = (0..s.total()).filter(|&i| s.ps[i].running).collect();
                if model.is_some() && (running.is_empty() || next() % 2 == 0) {
                    s.start(model.unwrap());
                } else if let Some(&i) = running.get((next() as usize) % running.len().max(1)) {
                    s.end(i, Ended { st: St::Progress, weight: None, service: 1 + next() % 4 });
                }
            }
        }
    }
}
