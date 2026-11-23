//! agree: the leaders agree on a plan before any launch, and every process of a host declares from it (INTERFACE.md
//! 6, CONTRACT.md F6). Every process asks `is_leader` (which has the hosts discover each other). Each leader but
//! host 0's sends its host and the workers it was supplied to host 0's leader with `peer_send`; host 0 waits for them
//! with `peer_wait`, builds the plan, one count per host, and sends it to every other leader. Each leader gives the
//! plan to the other processes of its host with `share`, which runs `agree` on the leader only. Every process declares the workers the plan gives its
//! host and launches; a worker reports the plan's length and its count back to its leader, which checks both.
//! Host 0 prints "host h: ok" for each host. Expected output: agree.expected (one host) and agree.h2.expected
//! (two); check/agree.sh runs it on hosts of different sizes.
use trame::{Bounded, HostId, Leader, Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

#[trame::message]
struct Hello {
    host: u32,
    workers: u32,
}

/// An object: its counts are a `Bounded`, sized when it is built, and it crosses between leaders and to the other
/// processes of a host with no byte written here.
#[derive(trame::Object)]
struct Plan {
    counts: Bounded<u32>,
    hosts: u32,
}

#[trame::message]
struct Seen {
    k: u32,
    hosts: u32,
}

#[trame::state(steps(report))]
struct Reporter {
    k: u32,
    hosts: u32,
}

#[trame::step(Reporter)]
fn report(k: &u32, hosts: &u32) -> Status {
    if trame::send(Leader, Seen { k: *k, hosts: *hosts }).is_err() {
        return Status::wait().room::<Seen>(Leader);
    }
    Status::Done
}

/// What the leaders agree: host 0 collects every host's supplied workers and answers the plan.
fn agree(me: HostId, hosts: u32) -> Plan {
    if me.0 == 0 {
        let mut counts = vec![0u32; hosts as usize];
        counts[0] = trame::workers(me).expect("supplied");
        let mut got = 1;
        while got < hosts {
            match trame::peer_recv::<Hello>() {
                Some((from, h)) => {
                    assert_eq!(from.0, h.host);
                    counts[h.host as usize] = h.workers;
                    got += 1;
                }
                None => trame::peer_wait(),
            }
        }
        let mut plan = Plan { counts: Bounded::new(hosts as usize), hosts };
        plan.counts.extend_from_slice(&counts).expect("one count per host");
        for h in 1..hosts {
            trame::peer_send(HostId(h), &plan).expect("peer send");
        }
        plan
    } else {
        trame::peer_send(HostId(0), &Hello { host: me.0, workers: trame::workers(me).expect("supplied") })
            .expect("peer send");
        loop {
            match trame::peer_recv::<Plan>() {
                Some((from, p)) => {
                    assert_eq!(from.0, 0, "the plan comes from host 0");
                    return p;
                }
                None => trame::peer_wait(),
            }
        }
    }
}

fn main() {
    let (me, hosts) = (trame::host(), trame::hosts());
    // Every process asks, which is collective over the world; only a leader has peers.
    let plan = trame::share(|| agree(me, hosts));
    assert_eq!(plan.hosts, hosts, "the plan reached a process of host {}", me.0);
    let n = plan.counts.as_slice()[me.0 as usize];
    assert!(n >= 1);
    let mut t = Topology::new();
    let ws: Vec<_> = (0..n).map(|_| t.worker::<Reporter>()).collect();
    for (k, w) in ws.iter().enumerate() {
        let (k, hosts) = (k as u32, plan.hosts);
        t.set(w, move || Reporter { k, hosts });
        t.route::<Seen>(w.id(), Leader, 1);
    }
    let run = t.launch().expect("launch");
    let mut seen = 0;
    while seen < n {
        match run.recv::<Seen>().map(|s| s.value) {
            Some(s) => {
                assert_eq!(s.hosts, hosts);
                seen += 1;
            }
            None => run.wait(),
        }
    }
    if me.0 == 0 {
        for h in 0..hosts {
            println!("host {h}: ok");
        }
    }
    run.join();
}
