//! Conformance of a backend's scheduler to the model (sched.rs), on random workloads.
//!
//! A workload is a state *shape* (fixed fields and steps with overlapping read and write sets) and a
//! seed. Each step owns a control field `cK` holding a PRNG and a budget; on each run it draws what to
//! return: progress (sometimes with a new weight), a bare wait, a timed wait, or done when its budget
//! is spent. So the seed decides the statuses and weights, and the shape decides who excludes and who
//! wakes whom. Each run records its trace; `__check_traces` replays it against the model: exactly
//! (choices included) on seq, safety only elsewhere (CONTRACT.md, S1-S6, S9).
//!
//! A failure prints the shape and the seed; add the seed to REGRESSIONS so every later run tries it.
//! Shares (S6) are checked by `shares` below.
#![cfg(trame_checked)]

use std::time::Duration;
use trame::{Outcome, Status, Topology};

/// A step's control: xorshift state, steps left, and how wild it is.
#[derive(Clone, Copy, Debug)]
struct Ctl {
    rng: u64,
    left: u32,
}

impl Ctl {
    fn new(seed: u64, k: u64) -> Ctl {
        let mut c = Ctl { rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (k + 1).wrapping_mul(0xD1B5_4A32_D192_ED03) | 1, left: 0 };
        c.left = 5 + (c.next() % 40) as u32;
        c
    }

    fn next(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

/// What step `c` returns this time. Waits never outlast the run: a bare wait is woken by a writer of
/// one of its fields, or ends in Deadlock when none is left, which the model allows.
fn behave(c: &mut Ctl) -> Status {
    if c.left == 0 {
        return Status::Done;
    }
    c.left -= 1;
    match c.next() % 10 {
        0..=5 => Status::Progress,
        6 | 7 => Status::Progress.weight((c.next() % 5) as f64 / 4.0),
        8 if c.next() % 2 == 0 => Status::wait(),
        8 => Status::Progress,
        _ => Status::wait().until(trame::now().after(Duration::from_micros(20 + c.next() % 200))),
    }
}

/// A shape: a state with fields f0..f3 (the shared data) and c0..cN (one control per step), and steps
/// that read and write given shared fields. `$rd` and `$wr` name shared fields.
macro_rules! shape {
    ($S:ident { $($step:ident ($c:ident; read $($rd:ident)*; write $($wr:ident)*)),* $(,)? }) => {
        #[trame::state(steps($($step),*))]
        #[allow(dead_code)]
        struct $S {
            f0: u64,
            f1: u64,
            f2: u64,
            f3: u64,
            $($c: Ctl,)*
        }
        $(
            #[trame::step($S)]
            #[allow(unused_variables)]
            fn $step($c: &mut Ctl, $($rd: &u64,)* $($wr: &mut u64,)*) -> Status {
                $(*$wr = (*$wr).wrapping_add(1);)*
                behave($c)
            }
        )*
        impl $S {
            fn seeded(seed: u64) -> $S {
                let mut k = 0u64;
                $S { f0: 0, f1: 0, f2: 0, f3: 0, $($c: { k += 1; Ctl::new(seed, k) },)* }
            }
        }
    };
}

// A chain: each step writes one field and reads its neighbour's.
shape!(Chain {
    a0(c0; read f1; write f0),
    a1(c1; read f2; write f1),
    a2(c2; read f3; write f2),
    a3(c3; read f0; write f3),
});

// Contention: every step writes f0; some read the others.
shape!(Hot {
    h0(c0; read; write f0),
    h1(c1; read f1; write f0),
    h2(c2; read f1 f2; write f0),
    h3(c3; read; write f0 f3),
    h4(c4; read f3; write f1),
});

// Readers and one writer: readers share, the writer excludes them.
shape!(Readers {
    r0(c0; read f0; write),
    r1(c1; read f0 f1; write),
    r2(c2; read f0; write),
    w0(c3; read; write f0),
    w1(c4; read f2; write f1),
});

/// Seeds every run tries: 0..SEEDS, then every seed in REGRESSIONS.
const SEEDS: u64 = 300;

/// Seeds that once failed: a failure prints its seed; add it here so it is tried on every run.
const REGRESSIONS: &[u64] = &[];

fn seeds() -> impl Iterator<Item = u64> {
    (0..SEEDS).chain(REGRESSIONS.iter().copied())
}

/// Totals over every workload: (events replayed, returned, deadlocked).
static EVENTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static RETURNED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static DEADLOCKED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Run one workload of shape `S` on the selected backend and replay its trace.
fn one<S: trame::__rt::State>(name: &str, seed: u64, state: S, exact: bool) {
    let mut t = Topology::new();
    t.__trace();
    let w = t.worker();
    t.set(&w, move || state);
    let run = t.launch().expect("launch");
    // A run that deadlocks is still a valid run; join says so.
    let (out, checked) = run.__join_checked(exact);
    assert!(matches!(out[0], Outcome::Returned | Outcome::Deadlock), "{name} seed {seed}: outcome {out:?}");
    let _ = w;
    use std::sync::atomic::Ordering::Relaxed;
    match checked {
        Ok(n) => EVENTS.fetch_add(n, Relaxed),
        Err((worker, v)) => panic!("{name} seed {seed}: worker {worker} violates the model: {v}"),
    };
    if out[0] == Outcome::Returned { &RETURNED } else { &DEADLOCKED }.fetch_add(1, Relaxed);
}

#[test]
fn traces_follow_the_model() {
    let exact = cfg!(feature = "seq");
    for seed in seeds() {
        one("Chain", seed, Chain::seeded(seed), exact);
        one("Hot", seed, Hot::seeded(seed), exact);
        one("Readers", seed, Readers::seeded(seed), exact);
    }
    use std::sync::atomic::Ordering::Relaxed;
    eprintln!(
        "conformance: exact={exact} events={} returned={} deadlocked={}",
        EVENTS.load(Relaxed),
        RETURNED.load(Relaxed),
        DEADLOCKED.load(Relaxed)
    );
}

/// The pool of `Pooled`: the leader keeps `all.len() == n`, so a step that sees otherwise read a
/// pool mid-edit (P2).
struct Pair {
    n: u64,
    all: Vec<u64>,
}

/// Edits: two steps read the pool (one also writes f0), one does not and only writes f0.
#[trame::state(steps(p0, p1, p2), pool = Pair)]
struct Pooled {
    f0: u64,
    c0: Ctl,
    c1: Ctl,
    c2: Ctl,
}

#[trame::step(Pooled)]
fn p0(c0: &mut Ctl, f0: &mut u64, pool: &Pair) -> Status {
    assert_eq!(pool.all.len() as u64, pool.n, "a step read the pool mid-edit");
    *f0 = f0.wrapping_add(1);
    behave(c0)
}

#[trame::step(Pooled)]
fn p1(c1: &mut Ctl, f0: &u64, pool: &Pair) -> Status {
    assert_eq!(pool.all.len() as u64, pool.n, "a step read the pool mid-edit");
    let _ = f0;
    behave(c1)
}

#[trame::step(Pooled)]
fn p2(c2: &mut Ctl, f0: &mut u64) -> Status {
    *f0 = f0.wrapping_add(1);
    behave(c2)
}

/// S4 and P2 under racing steps: two workers of `Pooled`; the leader edits the pool (growing a Vec)
/// and the workers' states while they run, and the traces, with their Hold and Edit events, are
/// replayed against the model.
#[test]
fn edits_follow_the_model() {
    use std::sync::atomic::Ordering::Relaxed;
    let exact = cfg!(feature = "seq");
    let (mut events, mut edits) = (0, 0);
    for seed in seeds() {
        let mut t = Topology::new();
        t.__trace();
        let pool = t.pool();
        t.set(&pool, || Pair { n: 0, all: Vec::new() });
        let ws: Vec<_> = (0..2u64)
            .map(|k| {
                let w = t.worker();
                let s = seed * 2 + k;
                t.set(&w, move || Pooled { f0: 0, c0: Ctl::new(s, 1), c1: Ctl::new(s, 2), c2: Ctl::new(s, 3) });
                w
            })
            .collect();
        let run = t.launch().expect("launch");
        let mut rng = Ctl::new(seed, 99);
        for _ in 0..20 {
            match rng.next() % 3 {
                0 => {
                    let w = &ws[(rng.next() % 2) as usize];
                    run.edit(w, |s| s.f0 = s.f0.wrapping_add(100));
                }
                _ => {
                    assert!(run.edit(&pool, |p| {
                        p.n += 1;
                        p.all.push(p.n);
                    }));
                }
            }
            edits += 1;
        }
        let (out, checked) = run.__join_checked(exact);
        for (k, o) in out.iter().enumerate() {
            assert!(matches!(o, Outcome::Returned | Outcome::Deadlock), "Pooled seed {seed}: worker {k} outcome {o:?}");
        }
        match checked {
            Ok(n) => events += n,
            Err((worker, v)) => panic!("Pooled seed {seed}: worker {worker} violates the model: {v}"),
        }
    }
    let _ = Relaxed;
    eprintln!("edits: exact={exact} events={events} leader edits={edits}");
}

/// S6: three steps that always progress and all write `hot`, with weights 1, 1/2 and 1/4, and one of
/// weight 0. Each counts its own runs. The leader stops the run after a while and takes the counts.
#[trame::state(steps(s1, s2, s4, s0))]
struct Contended {
    hot: u64,
    n1: u64,
    n2: u64,
    n4: u64,
    n0: u64,
}

#[trame::step(Contended, weight = 1.0)]
fn s1(hot: &mut u64, n1: &mut u64) -> Status {
    *hot += spin(2000);
    *n1 += 1;
    Status::Progress
}

#[trame::step(Contended, weight = 0.5)]
fn s2(hot: &mut u64, n2: &mut u64) -> Status {
    *hot += spin(2000);
    *n2 += 1;
    Status::Progress
}

#[trame::step(Contended, weight = 0.25)]
fn s4(hot: &mut u64, n4: &mut u64) -> Status {
    *hot += spin(2000);
    *n4 += 1;
    Status::Progress
}

#[trame::step(Contended)]
fn s0(hot: &mut u64, n0: &mut u64) -> Status {
    *hot += spin(2000);
    *n0 += 1;
    Status::Progress
}

#[test]
fn shares_follow_weights() {
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, move || Contended { hot: 0, n1: 0, n2: 0, n4: 0, n0: 0 });
    let run = t.launch().expect("launch");
    std::thread::sleep(Duration::from_millis(200));
    run.stop();
    assert_eq!(run.join(), [Outcome::Stopped]);
    let c = w.take();
    let total = (c.n1 + c.n2 + c.n4) as f64;
    let share = |n: u64| n as f64 / total;
    eprintln!(
        "shares: steps={} n1={} n2={} n4={} n0={} share1={:.4} share2={:.4} share4={:.4} (model 0.5714 0.2857 0.1429)",
        c.hot, c.n1, c.n2, c.n4, c.n0, share(c.n1), share(c.n2), share(c.n4)
    );
    assert!(total > 1000.0, "too few steps to judge: {total}");
    assert_eq!(c.n0, 0, "a weight-0 step ran while positive-weight steps were ready");
    // seq follows the model exactly: stride is off by at most one step per step.
    let tol = if cfg!(feature = "seq") { 3.0 / total } else { 0.02 };
    for (got, want) in [(share(c.n1), 4.0 / 7.0), (share(c.n2), 2.0 / 7.0), (share(c.n4), 1.0 / 7.0)] {
        assert!((got - want).abs() <= tol, "share {got:.4}, model {want:.4}, tolerance {tol:.4}");
    }
}

/// About `n` cheap operations, so that a step takes measurable time.
fn spin(n: u64) -> u64 {
    std::hint::black_box((0..n).fold(1u64, |a, x| a.wrapping_mul(6364136223846793005).wrapping_add(x)) & 1)
}

static RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MOST: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Count steps running at once (across the test's workers).
fn enter() {
    use std::sync::atomic::Ordering::SeqCst;
    let n = RUNNING.fetch_add(1, SeqCst) + 1;
    MOST.fetch_max(n, SeqCst);
}

fn leave() {
    RUNNING.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
}

/// Eight steps on eight disjoint fields: on a backend with several agents they must overlap.
#[trame::state(steps(d0, d1, d2, d3, d4, d5, d6, d7))]
struct Disjoint {
    x0: u64,
    x1: u64,
    x2: u64,
    x3: u64,
    x4: u64,
    x5: u64,
    x6: u64,
    x7: u64,
}

macro_rules! disjoint_step {
    ($($f:ident $x:ident),*) => {$(
        #[trame::step(Disjoint)]
        fn $f($x: &mut u64) -> Status {
            enter();
            *$x += 1 + spin(200_000);
            leave();
            if *$x >= 50 { Status::Done } else { Status::Progress }
        }
    )*};
}
disjoint_step!(d0 x0, d1 x1, d2 x2, d3 x3, d4 x4, d5 x5, d6 x6, d7 x7);

#[test]
fn disjoint_steps_overlap() {
    let mut t = Topology::new();
    t.__trace();
    let w = t.worker();
    t.set(&w, move || Disjoint { x0: 0, x1: 0, x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0 });
    let run = t.launch().expect("launch");
    let start = std::time::Instant::now();
    let (out, checked) = run.__join_checked(cfg!(feature = "seq"));
    let took = start.elapsed();
    assert_eq!(out, [Outcome::Returned]);
    checked.expect("the trace follows the model");
    let most = MOST.load(std::sync::atomic::Ordering::SeqCst);
    let s = w.take();
    eprintln!("overlap: steps={} most_at_once={most} wall_ms={:.1}", s.x0 + s.x1 + s.x2 + s.x3 + s.x4 + s.x5 + s.x6 + s.x7, took.as_secs_f64() * 1e3);
    if cfg!(feature = "seq") {
        assert_eq!(most, 1, "seq runs one step at a time");
    } else if std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) >= 8 {
        assert!(most >= 4, "eight disjoint steps on eight or more cores ran at most {most} at once");
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
