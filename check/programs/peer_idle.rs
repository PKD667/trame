//! peer_idle: a leader that waits for a peer event without one sleeps instead of spinning (INTERFACE.md 6,
//! Definition (Peer channel)). Two hosts: host 1's leader sends after 2 s, host 0's leader waits for that message in
//! `peer_wait`; the CPU its process spends over the wait separates a sleeping wait (a few milliseconds) from a
//! spinning one (about the whole wait). Threshold, so the line is the same on every machine: idle under 0.2 s.
//! Expected output: peer_idle.expected (one host) and peer_idle.h2.expected (two).

use trame::{HostId, Leader, Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

#[trame::message]
struct Ready {
    host: u32,
}

#[trame::state(steps(report))]
struct One {
    n: u32,
}

#[trame::step(One)]
fn report(n: &u32) -> Status {
    if trame::send(Leader, Ready { host: 0 }).is_err() {
        return Status::wait().room::<Ready>(Leader);
    }
    let _ = *n;
    Status::Done
}

/// This process's CPU time, user plus system, in ticks of 10 ms (the USER_HZ of every Linux here).
fn cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("/proc/self/stat");
    // The fields after the command name: state is the 3rd, utime the 14th and stime the 15th.
    let rest = &stat[stat.rfind(concat!(")")).expect("a command name") + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let field = |i: usize| f[i - 2].parse::<u64>().expect("a field of /proc/self/stat");
    field(14) + field(15)
}

fn main() {
    let (me, hosts) = (trame::host(), trame::hosts());
    let leader = trame::is_leader();
    if hosts >= 2 && leader {
        if me.0 == 0 {
            let before = cpu_ticks();
            loop {
                match trame::peer_recv::<Ready>() {
                    Some((from, r)) => {
                        assert_eq!((from.0, r.host), (1, 1));
                        break;
                    }
                    None => trame::peer_wait(),
                }
            }
            let spent = cpu_ticks() - before;
            println!("host 0: woke on the peer's message, cpu {}", if spent <= 20 { "idle" } else { "busy" });
        } else if me.0 == 1 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            trame::peer_send(HostId(0), &Ready { host: 1 }).expect("peer send");
            println!("host 1: sent after 2 s");
        }
    }
    // A launch on every host. The program exchanges peer messages before its launch, so the launcher does not ask it
    // for a rank count: it declares the workers the launcher gave its host (INTERFACE.md 6).
    let n = trame::workers(me).expect("supplied");
    let mut t = Topology::new();
    let ws: Vec<_> = (0..n).map(|_| t.worker::<One>()).collect();
    for w in &ws {
        t.set(w, || One { n: 0 });
        t.route::<Ready>(w.id(), Leader, 1);
    }
    let run = t.launch().expect("launch");
    let mut seen = 0;
    while seen < n as usize {
        match run.recv::<Ready>() {
            Some(_) => seen += 1,
            None => run.wait(),
        }
    }
    run.join();
    if leader {
        println!("launched host {}", me.0);
    }
}
