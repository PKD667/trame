//! share_refused: a builder that takes more than its worker's share refuses the launch (INTERFACE.md 6, Definition
//! (Memory of a host)), with the numbers. The launcher declares the host's memory (`TRAME_AVAILABLE`), which the
//! program sets here so the numbers are the same on every host: 16 MiB, one worker, so the share is a sixteenth less
//! that 16 MiB, 15728640 bytes, and a builder that takes 16 MiB goes past it. Expected output: share_refused.expected.

use trame::{Status, Topology};

#[trame::state(steps(idle))]
struct Held {
    big: trame::Bounded<u64>,
}

#[trame::step(Held)]
fn idle(big: &trame::Bounded<u64>) -> Status {
    let _ = big.len();
    Status::Done
}

fn main() {
    // SAFETY: nothing else in this process reads the environment before the launch reads this.
    unsafe { std::env::set_var("TRAME_AVAILABLE", "16777216") };
    let mut t = Topology::new();
    let w = t.worker::<Held>();
    t.set(&w, move || {
        let mut big = trame::Bounded::<u64>::new(1 << 21);
        for i in 0..(1 << 21) as u64 {
            big.push(i).expect("capacity is the length");
        }
        Held { big }
    });
    // The share is the declared memory less a sixteenth, over one worker: the same number on every host and
    // backend. How many bytes trame's own allocations add to a builder's count is not frozen here, only that the
    // builder's want is above the share.
    let leader = trame::is_leader();
    match t.launch() {
        Ok(_) => println!("launched"),
        // The refusal is the same on every process of the host; the leader prints it once.
        Err(trame::Refused::Share { want, share, .. }) if leader => {
            println!("share: {share}");
            println!("refused: exceeded: {}", want > share);
        }
        Err(trame::Refused::Share { want, share, .. }) => {
            assert!(want > share, "every process refuses the same launch");
        }
        Err(e) if leader => println!("refused: {e:?}"),
        Err(_) => {}
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs).
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
