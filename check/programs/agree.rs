//! agree: the leaders agree on a plan before any launch, and every process of a host declares from it (INTERFACE.md
//! 6, CONTRACT.md F6). Every process asks `is_leader` (which has the hosts discover each other). Each leader but
//! host 0's sends its host and the workers it was supplied to host 0's leader with `peer_send`; host 0 waits for them
//! with `peer_wait`, builds the plan, one count per host, and sends it to every other leader. Each leader gives the
//! plan to the other processes of its host with `share`. Every process declares the workers the plan gives its
//! host and launches; a worker reports the plan's length and its count back to its leader, which checks both.
//! Host 0 prints "host h: ok" for each host. Expected output: agree.expected (one host) and agree.h2.expected
//! (two); check/agree.sh runs it on hosts of different sizes.
use trame::{HostId, Leader, Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

#[trame::message]
struct Hello {
    host: u32,
    workers: u32,
}

#[trame::message]
struct Plan {
    counts: [u32; 8],
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

/// The plan as bytes, the form that `share` carries.
fn encode(p: &Plan) -> Vec<u8> {
    let mut b: Vec<u8> = p.counts.iter().flat_map(|c| c.to_le_bytes()).collect();
    b.extend_from_slice(&p.hosts.to_le_bytes());
    b
}

fn decode(b: &[u8]) -> Plan {
    let word = |i: usize| u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    Plan { counts: std::array::from_fn(word), hosts: word(8) }
}

/// What the leaders agree: host 0 collects every host's supplied workers and answers the plan.
fn agree(me: HostId, hosts: u32) -> Plan {
    if me.0 == 0 {
        let mut plan = Plan { counts: [0; 8], hosts };
        plan.counts[0] = trame::workers(me).expect("supplied");
        let mut got = 1;
        while got < hosts {
            match trame::peer_recv::<Hello>() {
                Some((from, h)) => {
                    assert_eq!(from.0, h.host);
                    plan.counts[h.host as usize] = h.workers;
                    got += 1;
                }
                None => trame::peer_wait(),
            }
        }
        for h in 1..hosts {
            trame::peer_send(HostId(h), Plan { counts: plan.counts, hosts }).expect("peer send");
        }
        plan
    } else {
        trame::peer_send(HostId(0), Hello { host: me.0, workers: trame::workers(me).expect("supplied") })
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
    let plan = if trame::is_leader() { encode(&agree(me, hosts)) } else { Vec::new() };
    let plan = decode(&trame::share(&plan));
    assert_eq!(plan.hosts, hosts, "the plan reached a process of host {}", me.0);
    let n = plan.counts[me.0 as usize];
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
