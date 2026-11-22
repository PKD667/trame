//! bench_step PER K N: the cost of a step that does nothing. Each host runs PER workers; each worker has K
//! steps (1 or 4) on disjoint fields, and each step counts to N, returning Progress each time. A step takes a
//! reading on its first call and on its last; ns / N is the time from one start of the step to the next on
//! that worker's clock: the agent's loop, the model's choice and the step's call. With K = 4 the steps
//! may run on several agents at once. Each leader prints one JSON line per step of its host, then its
//! outcomes. Built with `--cfg trame_checked`.

#[path = "common.rs"]
mod common;

use trame::{Leader, Reading, Status, Topology, Worker};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    worker: u64,
    step: u64,
    ns: u64,
}

/// One counter: its count, its first reading, and whether its report is out.
#[derive(Clone, Copy, trame::Plain)]
struct Count {
    n: u64,
    t0: Option<Reading>,
    ns: Option<u64>,
    reported: bool,
}

fn count(c: &mut Count, worker: u64, step: u64, n: u64) -> Status {
    if c.reported {
        return Status::Done;
    }
    if let Some(ns) = c.ns {
        return match trame::send(Leader, Report { worker, step, ns }) {
            Ok(()) => {
                c.reported = true;
                Status::Done
            }
            Err(_) => Status::wait().room::<Report>(Leader),
        };
    }
    let now = trame::now();
    let t0 = *c.t0.get_or_insert(now);
    c.n += 1;
    if c.n == n {
        c.ns = Some(common::ns(now.since(t0).expect("one worker has one clock")));
    }
    Status::Progress
}

const ZERO: Count = Count { n: 0, t0: None, ns: None, reported: false };

#[trame::state(steps(one_a))]
struct One {
    me: u64,
    n: u64,
    a: Count,
}

#[trame::step(One)]
fn one_a(me: &u64, n: &u64, a: &mut Count) -> Status {
    count(a, *me, 0, *n)
}

#[trame::state(steps(four_a, four_b, four_c, four_d))]
struct Four {
    me: u64,
    n: u64,
    a: Count,
    b: Count,
    c: Count,
    d: Count,
}

#[trame::step(Four)]
fn four_a(me: &u64, n: &u64, a: &mut Count) -> Status {
    count(a, *me, 0, *n)
}
#[trame::step(Four)]
fn four_b(me: &u64, n: &u64, b: &mut Count) -> Status {
    count(b, *me, 1, *n)
}
#[trame::step(Four)]
fn four_c(me: &u64, n: &u64, c: &mut Count) -> Status {
    count(c, *me, 2, *n)
}
#[trame::step(Four)]
fn four_d(me: &u64, n: &u64, d: &mut Count) -> Status {
    count(d, *me, 3, *n)
}

fn main() {
    let per: u64 = common::arg(1, "PER");
    let k: u64 = common::arg(2, "K");
    let n: u64 = common::arg(3, "N");
    if per == 0 || n < 2 || !(k == 1 || k == 4) {
        eprintln!("usage: bench_step PER>=1 K in {{1,4}} N>=2");
        std::process::exit(2);
    }
    let (me, hosts) = (trame::host().0, trame::hosts());
    let mut t = Topology::new();
    for w in 0..per {
        let id = if k == 1 {
            let h: Worker<One> = t.worker();
            t.set(&h, move || One { me: w, n, a: ZERO });
            h.id()
        } else {
            let h: Worker<Four> = t.worker();
            t.set(&h, move || Four { me: w, n, a: ZERO, b: ZERO, c: ZERO, d: ZERO });
            h.id()
        };
        t.route::<Report>(id, Leader, 4);
    }
    let run = t.launch().unwrap_or_else(|e| common::refused(e));
    common::collect::<Report>(&run, (per * k) as usize, |r| {
        println!(
            "{{\"bench\":\"step\",\"hosts\":{hosts},\"host\":{me},\"per\":{per},\"k\":{k},\"n\":{n},\"worker\":{},\"step\":{},\"ns\":{}}}",
            r.worker, r.step, r.ns
        );
    });
    common::finish(run);
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
