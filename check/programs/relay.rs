//! relay: leaders exchange byte messages of any length, in both directions at once, before and after the launch
//! (INTERFACE.md 6, CONTRACT.md F6). Every leader sends every other leader a 3 MiB message (above any eager limit,
//! so a blocking send would wait for its receiver) and a short one, both before its launch, and only then receives:
//! two leaders that sent to each other would deadlock if `peer_send_bytes` waited. Each checks every byte of what it
//! got. After the launch the same exchange is made again with `Run::leader_send`'s byte counterpart. A host that is
//! not a leader waits in `share` while its leader is busy, and gets the leader's checksum back. Host 0 prints
//! "host h: ok" for each host. Expected output: relay.expected (one host) and relay.h2.expected (two).
use trame::{HostId, Leader, Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

const BIG: usize = 3 << 20;
const BIG_TAG: u64 = 0x5eed_0001;
const SMALL_TAG: u64 = 0x5eed_0002;
const LATE_TAG: u64 = 0x5eed_0003;

#[trame::message]
struct Done {
    ok: u32,
}

#[trame::state(steps(report))]
struct Reporter {
    k: u32,
}

#[trame::step(Reporter)]
fn report(k: &u32) -> Status {
    if trame::send(Leader, Done { ok: *k + 1 }).is_err() {
        return Status::wait().room::<Done>(Leader);
    }
    Status::Done
}

/// The bytes host `from` sends: its number repeated with a counter, so a lost or crossed byte shows.
fn pattern(from: u32, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u32).wrapping_mul(31).wrapping_add(from) as u8).collect()
}

/// Send every peer the three messages, then receive every peer's and check them all.
fn exchange(me: HostId, hosts: u32, tags: &[(u64, usize)], wait: &dyn Fn()) {
    for h in (0..hosts).filter(|&h| h != me.0) {
        for &(tag, len) in tags {
            trame::peer_send_bytes(HostId(h), tag, &pattern(me.0, len));
        }
    }
    for &(tag, len) in tags {
        let mut from = std::collections::BTreeSet::new();
        while from.len() < hosts as usize - 1 {
            match trame::peer_recv_bytes(tag) {
                Some((h, bytes)) => {
                    assert_eq!(bytes, pattern(h.0, len), "message {tag:#x} of host {} is not what it sent", h.0);
                    assert!(from.insert(h.0), "host {} sent {tag:#x} twice", h.0);
                }
                None => wait(),
            }
        }
    }
}

fn main() {
    let (me, hosts) = (trame::host(), trame::hosts());
    // Every process asks, collectively; the leader works while the others sleep in `share`.
    let leader = trame::is_leader();
    if leader {
        exchange(me, hosts, &[(BIG_TAG, BIG), (SMALL_TAG, 5)], &trame::peer_wait);
    }
    let sum = trame::share(&[leader as u8; 1]);
    assert_eq!(sum, [1], "a process of host {} was not given its leader's bytes", me.0);
    // The host declares the workers the launcher gave it: this program exchanges before its launch, so it is not asked.
    let n = trame::workers(me).expect("supplied");
    let mut t = Topology::new();
    let ws: Vec<_> = (0..n).map(|_| t.worker::<Reporter>()).collect();
    for w in &ws {
        t.set(w, || Reporter { k: 0 });
        t.route::<Done>(w.id(), Leader, 1);
    }
    let run = t.launch().expect("launch");
    // After the launch: the same exchange, with a message sent while the other leader is still in the run.
    exchange(me, hosts, &[(LATE_TAG, BIG)], &|| run.wait());
    let mut seen = 0;
    while seen < n {
        match run.recv::<Done>() {
            Some(d) => {
                assert_eq!(d.value.ok, 1);
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
