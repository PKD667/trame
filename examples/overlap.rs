//! overlap: do steps of one worker on disjoint fields run at once? One worker, 16 steps, each on its own
//! field, each running `rounds` times a fixed amount of work. The leader runs it with 1, 2, 4, 8, 16 and
//! 32 agents (a test hook; the default is the cores) and prints the wall time from the return of launch
//! to join and
//! the speedup over one agent. Arguments: rounds per step (default 40), and the one agent count to
//! run, for a backend where a launch is once per process (below).
//!
//! An experiment, not a program with an .expected file: its numbers are measurements.

use std::time::Instant;
use trame::{Outcome, Status, Topology};

#[trame::state(steps(s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11, s12, s13, s14, s15))]
struct Disjoint {
    rounds: u64,
    x0: u64, x1: u64, x2: u64, x3: u64, x4: u64, x5: u64, x6: u64, x7: u64,
    x8: u64, x9: u64, x10: u64, x11: u64, x12: u64, x13: u64, x14: u64, x15: u64,
}

fn spin() -> u64 {
    std::hint::black_box((0..200_000u64).fold(1u64, |x, i| x.wrapping_mul(6364136223846793005).wrapping_add(i)) & 1)
}

macro_rules! steps {
    ($($s:ident $x:ident),*) => {$(
        #[trame::step(Disjoint)]
        fn $s(rounds: &u64, $x: &mut u64) -> Status {
            *$x += 1 + spin();
            if *$x >= *rounds { Status::Done } else { Status::Progress }
        }
    )*};
}
steps!(s0 x0, s1 x1, s2 x2, s3 x3, s4 x4, s5 x5, s6 x6, s7 x7, s8 x8, s9 x9, s10 x10, s11 x11, s12 x12, s13 x13, s14 x14, s15 x15);

fn main() {
    let rounds: u64 = std::env::args().nth(1).map(|s| s.parse().expect("rounds")).unwrap_or(40);
    // On a backend whose workers are processes of their own, a worker process does not return from
    // launch, so one process runs one launch: pass the agent count as the second argument there.
    let only: Option<usize> = std::env::args().nth(2).map(|s| s.parse().expect("agents"));
    let counts: Vec<usize> = match only {
        Some(n) => vec![n],
        None => vec![1, 2, 4, 8, 16, 32],
    };
    println!("agents\twall_ms\tspeedup");
    let mut one = None;
    for agents in counts {
        let mut t = Topology::new();
        t.__agents(agents);
        let _w = t.worker();
        t.set(&_w, move || Disjoint {
            rounds, x0: 0, x1: 0, x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
            x8: 0, x9: 0, x10: 0, x11: 0, x12: 0, x13: 0, x14: 0, x15: 0,
        });
        // After launch: on a backend with workers in other processes, launch includes their start and the agreement on the plan.
        let run = t.launch().expect("launch");
        let start = Instant::now();
        assert_eq!(run.join(), [Outcome::Returned]);
        let ms = start.elapsed().as_secs_f64() * 1e3;
        let base = *one.get_or_insert(ms);
        println!("{agents}\t{ms:.1}\t{:.2}", base / ms);
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
