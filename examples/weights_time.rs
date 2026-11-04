//! weights_time: are a worker's steps given time in proportion to their weights? One worker, three
//! steps that all write `hot`, so at most one runs at a time; weights 1, 1/2 and 1/4, and costs that
//! differ on purpose (step a spins 4 units, b 1, c 2), so counting steps would not show it. Each step
//! adds the nanoseconds it ran to its own counter. The leader stops the run after the seconds given as
//! the first argument (default 2); the stop step sends the totals, and the leader prints one line per
//! step: weight, steps, nanoseconds, time share, model share (w / sum w).
//!
//! An experiment, not a program with an .expected file: its numbers are measurements.

use std::time::Duration;
use trame::{Leader, Outcome, Status, Topology};

#[trame::message]
#[derive(Debug)]
struct Totals {
    steps: [u64; 3],
    nanos: [u64; 3],
}

#[trame::state(steps(a, b, c), stop = report)]
struct Contended {
    hot: u64,
    na: [u64; 2],
    nb: [u64; 2],
    nc: [u64; 2],
}

fn spin(units: u64) -> u64 {
    std::hint::black_box((0..units * 5000).fold(1u64, |x, i| x.wrapping_mul(6364136223846793005).wrapping_add(i)) & 1)
}

/// Run `units` of work, and add one step and its nanoseconds to `n`.
fn work(hot: &mut u64, n: &mut [u64; 2], units: u64) -> Status {
    let t0 = trame::now();
    *hot += spin(units);
    n[0] += 1;
    n[1] += trame::now().since(t0).as_nanos() as u64;
    Status::Progress
}

#[trame::step(Contended, weight = 1.0)]
fn a(hot: &mut u64, na: &mut [u64; 2]) -> Status {
    work(hot, na, 4)
}

#[trame::step(Contended, weight = 0.5)]
fn b(hot: &mut u64, nb: &mut [u64; 2]) -> Status {
    work(hot, nb, 1)
}

#[trame::step(Contended, weight = 0.25)]
fn c(hot: &mut u64, nc: &mut [u64; 2]) -> Status {
    work(hot, nc, 2)
}

#[trame::step(Contended)]
fn report(na: &[u64; 2], nb: &[u64; 2], nc: &[u64; 2]) -> Status {
    let t = Totals { steps: [na[0], nb[0], nc[0]], nanos: [na[1], nb[1], nc[1]] };
    match trame::send(Leader, t) {
        Ok(()) => Status::Done,
        Err(_) => Status::wait().room::<Totals>(Leader),
    }
}

fn main() {
    let secs: f64 = std::env::args().nth(1).map(|s| s.parse().expect("seconds")).unwrap_or(2.0);
    let mut t = Topology::new();
    let w = t.worker();
    t.set(&w, move || Contended { hot: 0, na: [0; 2], nb: [0; 2], nc: [0; 2] });
    t.route::<Totals>(w.id(), Leader, 1);
    let run = t.launch().expect("launch");
    std::thread::sleep(Duration::from_secs_f64(secs));
    run.stop();
    let totals = loop {
        if let Some(x) = run.recv::<Totals>() {
            break x;
        }
        run.wait();
    };
    assert_eq!(run.join(), [Outcome::Stopped]);
    let weights = [1.0, 0.5, 0.25];
    let all: u64 = totals.nanos.iter().sum();
    println!("step\tweight\tsteps\tnanos\ttime_share\tmodel_share");
    for k in 0..3 {
        println!(
            "{}\t{}\t{}\t{}\t{:.4}\t{:.4}",
            ["a", "b", "c"][k],
            weights[k],
            totals.steps[k],
            totals.nanos[k],
            totals.nanos[k] as f64 / all as f64,
            weights[k] / 1.75
        );
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
