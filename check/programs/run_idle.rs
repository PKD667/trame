//! run_idle: a leader waiting with peers naps between polls of the peer channel instead of spinning (INTERFACE.md 6,
//! Definition (Peer channel)). Two hosts, and a run that cannot move: the worker fills its route to the leader and
//! waits for room, and the leader waits for a message it never receives, so the detector reaches E2's verdict. The
//! leader reads its own CPU time over that wait from /proc/self/stat: a nap spends almost none, a spin spends the
//! window. Threshold, so the line is the same on every machine: idle under 0.2 s of CPU.
//! Expected output: run_idle.h2.expected (two hosts; one host has nothing to wait for, and run.sh skips it).
//!
//! What this check can and cannot show: on one machine the detector reaches its verdict in well under the 0.2 s
//! threshold, so a spin there burns too few ticks to be told from a nap, and the check cannot discriminate. On two
//! machines the detector's rounds take longer, the window is real, and a spin spends it (this is the shape of the
//! stall a two-node run shows: a leader at a full core). The threshold line is the same either way.

use trame::{Leader, Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

#[trame::message]
struct Fill {
    n: u32,
}

#[trame::state(steps(fill))]
struct One {
    n: u32,
}

#[trame::step(One)]
fn fill(n: &mut u32) -> Status {
    // One message fits the route (capacity 1); the second waits for room that no leader will take.
    *n += 1;
    if trame::send(Leader, Fill { n: *n }).is_err() {
        return Status::wait().room::<Fill>(Leader);
    }
    if *n < 2 {
        return Status::Progress;
    }
    Status::Done
}

/// This process's CPU time, user plus system, in ticks of 10 ms (the USER_HZ of every Linux here).
fn cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("/proc/self/stat");
    let rest = &stat[stat.rfind(concat!(")")).expect("a command name") + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let field = |i: usize| f[i - 2].parse::<u64>().expect("a field of /proc/self/stat");
    field(14) + field(15)
}

fn main() {
    let (me, hosts) = (trame::host(), trame::hosts());
    let leader = trame::is_leader();
    let n = trame::workers(me).expect("supplied");
    let mut t = Topology::new();
    let ws: Vec<_> = (0..n).map(|_| t.worker::<One>()).collect();
    for w in &ws {
        t.set(w, || One { n: 0 });
        t.route::<Fill>(w.id(), Leader, 1);
    }
    let run = t.launch().expect("launch");
    if hosts >= 2 && leader && me.0 == 0 {
        let before = cpu_ticks();
        // Neither a message nor a verdict yet: wait until the detector makes E2's, and time the waiting.
        let stuck = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loop {
                if run.recv::<Fill>().is_none() {
                    run.wait();
                }
            }
        }));
        let spent = cpu_ticks() - before;
        assert!(stuck.is_err(), "the run cannot move, so the detector reaches a verdict");
        println!("host 0: deadlock reached, cpu {}", if spent <= 20 { "idle" } else { "busy" });
        return;
    }
    run.join();
    if leader {
        println!("launched host {}", me.0);
    }
}
