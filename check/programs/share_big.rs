//! share_big: the domain is sized from the host's memory at the launch, so a builder on a host whose memory is larger
//! than the domain's old fixed 16 GiB can spend its whole share (INTERFACE.md 6, Definition (Memory of a host)). The
//! launcher declares 64 GiB of host memory over two workers, so each share is 30 GiB, above the domain's old fixed
//! 16 GiB: the shape a 187 GB node has, a share no fixed domain could hold. Each builder reserves exactly `left()` bytes; reserving
//! touches no page, so the run costs no memory. Expected output: share_big.expected.

use trame::{Status, Topology};

/// Two workers, so each one's share (declared less the launch's sixteenth, over two) is above the domain's old
/// default of 16 GiB: a domain not sized from the host's memory cannot hold it.
const DECLARED: u64 = 64 << 30;

#[trame::message]
struct Seen {
    ok: u32,
}

#[trame::state(steps(report))]
struct Spent {
    bytes: u64,
}

#[trame::step(Spent)]
fn report(bytes: &u64) -> Status {
    let _ = *bytes;
    if trame::send(trame::Leader, Seen { ok: 1 }).is_err() {
        return Status::wait().room::<Seen>(trame::Leader);
    }
    Status::Done
}

fn main() {
    // SAFETY: nothing else in this process reads the environment before the launch reads this (CONTRACT.md L3).
    unsafe { std::env::set_var("TRAME_AVAILABLE", DECLARED.to_string()) };
    let leader = trame::is_leader();
    // Two workers whatever the machine has: the output is the same everywhere.
    let n = 2usize;
    let mut t = Topology::new();
    let ws: Vec<_> = (0..n).map(|_| t.worker::<Spent>()).collect();
    for w in &ws {
        t.set(w, move || {
            let left = trame::system::left().expect("left in a builder");
            // Reserve all of it, touching no page: the domain must be able to hold it.
            let mut room = trame::Bounded::<u64>::new(usize::try_from(left / 8).expect("a share of this host"));
            room.truncate(0);
            Spent { bytes: left }
        });
        t.route::<Seen>(w.id(), trame::Leader, 1);
    }
    let run = t.launch().expect("a builder that spends exactly left() fits the domain");
    let mut seen = 0;
    while seen < n {
        match run.recv::<Seen>() {
            Some(_) => seen += 1,
            None => run.wait(),
        }
    }
    run.join();
    if leader {
        println!("spent left() in every builder: {n}");
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs).
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
