//! relay: leaders exchange objects of any size, in both directions at once, before and after the launch
//! (INTERFACE.md 6, CONTRACT.md F6). Every leader sends every other leader a 3 MiB message (above any eager limit,
//! so a blocking send would wait for its receiver) and a short one, both before its launch, and only then receives:
//! two leaders that sent to each other would deadlock if `peer_send` waited. Each checks every byte of what it
//! got. After the launch the same exchange is made again with the object types of the run. A host that is
//! not a leader waits in `share` while its leader is busy, and gets the leader's value back. Host 0 prints
//! "host h: ok" for each host. Expected output: relay.expected (one host) and relay.h2.expected (two).
use trame::{Bounded, HostId, Leader, Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

const BIG: usize = 3 << 20;

/// Three object types of one shape: a sender and a `Bounded` of bytes whose capacity is its length. Each is its own
/// type, so each travels as its own stream of objects.
macro_rules! blob {
    ($($name:ident),*) => {$(
        #[derive(trame::Object)]
        struct $name {
            from: u32,
            data: Bounded<u8>,
        }
        impl Blob for $name {
            fn make(from: u32, data: Bounded<u8>) -> Self {
                $name { from, data }
            }
            fn view(&self) -> (u32, &[u8]) {
                (self.from, self.data.as_slice())
            }
        }
    )*};
}
trait Blob: trame::Object {
    fn make(from: u32, data: Bounded<u8>) -> Self;
    fn view(&self) -> (u32, &[u8]);
}
blob!(Big, Small, Late);

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

/// Send every peer an object of type `T` and `len` bytes, then receive every peer's and check it.
fn exchange<T: Blob>(me: HostId, hosts: u32, len: usize, wait: &dyn Fn()) {
    for h in (0..hosts).filter(|&h| h != me.0) {
        let mut data = Bounded::new(len);
        data.extend_from_slice(&pattern(me.0, len)).expect("capacity is the length");
        trame::peer_send(HostId(h), &T::make(me.0, data)).expect("peer send");
    }
    let mut from = std::collections::BTreeSet::new();
    while from.len() < hosts as usize - 1 {
        match trame::peer_recv::<T>() {
            Some((h, obj)) => {
                let (said, bytes) = obj.view();
                assert_eq!(said, h.0);
                assert_eq!(bytes, &pattern(h.0, len)[..], "the object of host {} is not what it sent", h.0);
                assert!(from.insert(h.0), "host {} sent twice", h.0);
            }
            None => wait(),
        }
    }
}

fn main() {
    let (me, hosts) = (trame::host(), trame::hosts());
    // Every process asks, collectively; the leader works while the others sleep in `share`.
    let leader = trame::is_leader();
    // Both are sent before either is received: with a send that waited, two leaders would be stuck.
    if leader {
        for h in (0..hosts).filter(|&h| h != me.0) {
            let mut data = Bounded::new(5);
            data.extend_from_slice(&pattern(me.0, 5)).expect("capacity is the length");
            trame::peer_send(HostId(h), &Small::make(me.0, data)).expect("peer send");
        }
        exchange::<Big>(me, hosts, BIG, &trame::peer_wait);
        let mut from = 0;
        while from < hosts - 1 {
            match trame::peer_recv::<Small>() {
                Some((h, obj)) => {
                    assert_eq!(obj.view().1, &pattern(h.0, 5)[..]);
                    from += 1;
                }
                None => trame::peer_wait(),
            }
        }
    }
    let said = trame::share(|| (me.0, leader));
    assert_eq!(said, (me.0, true), "a process of host {} was not given its leader's value", me.0);
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
    exchange::<Late>(me, hosts, BIG, &|| run.wait());
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
