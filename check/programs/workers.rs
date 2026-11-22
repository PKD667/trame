//! workers: `trame::workers(h)` is supplied by the launcher, before the program declares anything, the same on
//! every host (INTERFACE.md 6). Each host declares min(workers(h), 6) workers, so a launch that gives hosts
//! different counts gives them different topologies. Every worker reports `workers(host())` to its leader,
//! and the leader checks it equals what it read itself, and that it declared as many workers as it reported.
//! Each leader but host 0's sends its table of `workers(h)` for every h to host 0's leader, which checks it
//! equals its own and prints "host h: ok" for each host in order. Expected output: workers.expected (one host)
//! and workers.h2.expected (two); check/workers.sh runs it on hosts of different sizes.
use trame::{HostId, Leader, Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

/// The most workers a host declares, so that a machine with many cores still runs the program quickly.
const CAP: u32 = 6;

#[trame::message]
struct Seen {
    k: u32,
    supplied: u32,
}

/// The table of host `host`: `workers(h)` for h = 0..8, 0 beyond the hosts there are.
#[trame::message]
struct Table {
    host: u32,
    counts: [u32; 8],
}

#[trame::state(steps(report))]
struct Reporter {
    k: u32,
}

#[trame::step(Reporter)]
fn report(k: &u32) -> Status {
    if trame::send(Leader, Seen { k: *k, supplied: trame::workers(trame::host()) }).is_err() {
        return Status::wait().room::<Seen>(Leader);
    }
    Status::Done
}

fn table() -> [u32; 8] {
    let mut counts = [0; 8];
    for h in 0..trame::hosts().min(8) {
        counts[h as usize] = trame::workers(HostId(h));
    }
    counts
}

fn main() {
    let me = trame::host();
    // Before anything is declared: the count of this host and of every other.
    let mine = table();
    assert!(mine[me.0 as usize] >= 1);
    let n = mine[me.0 as usize].min(CAP);
    let mut t = Topology::new();
    let ws: Vec<_> = (0..n).map(|_| t.worker::<Reporter>()).collect();
    for (k, w) in ws.iter().enumerate() {
        let k = k as u32;
        t.set(w, move || Reporter { k });
        t.route::<Seen>(w.id(), Leader, 1);
    }
    let run = t.launch().expect("launch");
    let mut seen = 0;
    while seen < n {
        match run.recv::<Seen>().map(|s| s.value) {
            Some(s) => {
                assert_eq!(s.supplied, mine[me.0 as usize], "a worker was supplied another count than its leader");
                seen += 1;
            }
            None => run.wait(),
        }
    }
    if me.0 == 0 {
        let hosts = trame::hosts();
        let mut got = vec![false; hosts as usize];
        got[0] = true;
        while got.iter().any(|g| !g) {
            match run.leader_recv::<Table>() {
                Some((from, t)) => {
                    assert_eq!(from.0, t.host);
                    assert_eq!(t.counts, mine, "host {} was supplied another table of workers", t.host);
                    got[t.host as usize] = true;
                }
                None => run.wait(),
            }
        }
        for h in 0..hosts {
            println!("host {h}: ok");
        }
    } else {
        run.leader_send(HostId(0), Table { host: me.0, counts: mine }).expect("peer send");
    }
    run.join();
}
