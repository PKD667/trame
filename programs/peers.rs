//! Leaders of a federation as peers (CONTRACT.md F1, F6): every host runs one worker that counts to
//! ten from its host's number times 100. Each leader except host 0's sends its worker's count to host
//! 0's leader on the peer channel; host 0's leader prints every count, in host order. On one host, it
//! prints its own.
use trame::{Status, Topology};

#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);

#[trame::state(steps(count))]
struct Counter {
    n: u64,
    left: u32,
}

#[trame::step(Counter)]
fn count(n: &mut u64, left: &mut u32) -> Status {
    *n += 1;
    *left -= 1;
    if *left == 0 { Status::Done } else { Status::Progress }
}

#[trame::message]
struct Count {
    host: u32,
    n: u32,
}

fn main() {
    let me = trame::host();
    let mut t = Topology::new();
    let w = t.worker::<Counter>();
    t.set(&w, move || Counter { n: u64::from(me.0) * 100, left: 10 });
    let run = t.launch().expect("launch");
    // The worker's result is read from the leader only after join; the count it reaches is known.
    let mine = u64::from(me.0) * 100 + 10;
    if me.0 == 0 {
        let hosts = trame::hosts();
        let mut got = vec![None; hosts as usize];
        got[0] = Some(mine);
        while got.iter().any(Option::is_none) {
            match run.leader_recv::<Count>() {
                Some((from, c)) => {
                    assert_eq!(from.0, c.host);
                    got[c.host as usize] = Some(u64::from(c.n));
                }
                None => run.wait(),
            }
        }
        for (h, n) in got.iter().enumerate() {
            println!("host {h}: {}", n.unwrap());
        }
    } else {
        run.leader_send(trame::HostId(0), Count { host: me.0, n: mine as u32 }).expect("peer send");
    }
    run.join();
    assert_eq!(w.take().n, mine);
}
