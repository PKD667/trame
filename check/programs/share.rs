//! share: `trame::system::info()` (INTERFACE.md 6, Definition (Memory of a host)). Before the launch the share is
//! `None`; a builder reads it as a number, and a second reading in the same builder is the same number; a step reads
//! the same number the launch fixed. The builder takes 1 MiB of state, which every host can hold. Expected output:
//! share.expected.

use trame::{Status, Topology};

#[trame::message]
struct Seen {
    before: u64,
    share_hi: u64,
    step_same: u64,
}

#[trame::state(steps(report))]
struct St {
    share: u64,
    taken: u64,
}

#[trame::step(St)]
fn report(share: &u64, taken: &u64) -> Status {
    let now = trame::system::info().worker_bytes;
    let same = u64::from(now == Some(core::hint::black_box(*share)));
    if trame::send(trame::Leader, Seen { before: 0, share_hi: *taken, step_same: same }).is_err() {
        return Status::wait().room::<Seen>(trame::Leader);
    }
    Status::Done
}

fn main() {
    let before = trame::system::info().worker_bytes;
    // Read before the run: `join` ends the run, and a multi-process backend has finalized by the time it returns.
    let leader = trame::is_leader();
    let mut t = Topology::new();
    let w = t.worker::<St>();
    t.set(&w, move || {
        let first = trame::system::info().worker_bytes.expect("a builder reads its share");
        let again = trame::system::info().worker_bytes;
        assert_eq!(again, Some(first), "two readings in one builder differ");
        assert!(first >= 1 << 20, "a share below 1 MiB: {first}");
        // 1 MiB of state, inside the share on any host.
        let mut scratch = trame::Bounded::<u64>::new(1 << 17);
        for i in 0..(1 << 17) as u64 {
            scratch.push(i).expect("capacity is the length");
        }
        St { share: first, taken: scratch.len() as u64 }
    });
    t.route::<Seen>(w.id(), trame::Leader, 1);
    let run = t.launch().expect("launch");
    let mut step_same = u64::MAX;
    while step_same == u64::MAX {
        match run.recv::<Seen>() {
            Some(s) => step_same = s.value.step_same,
            None => run.wait(),
        }
    }
    run.join();
    // The share is the host's, and every process of the host reads the same one; the leader prints it once.
    if leader {
        println!("before: {}", if before.is_none() { "none".to_string() } else { "some".to_string() });
        println!("builder: some");
        println!("step reads the launch's share: {step_same}");
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs).
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
