//! left: `trame::system::left()` (INTERFACE.md 6, Definition (Memory of a host)). In a builder it is the share less
//! what the builder has taken, by the meter the refusal uses; a builder that sizes its last structure with it spends
//! the share exactly and the launch is not refused. A step reads `None`: the meter belongs to a builder. The launcher
//! declares the host's memory, so the numbers are the same on every host: 32 MiB, one worker, a sixteenth kept.
//! Expected output: left.expected.

use trame::{Status, Topology};

const SHARE: u64 = 32 * 1024 * 1024 - 32 * 1024 * 1024 / 16;

#[trame::message]
struct Seen {
    exact: u32,
    step_none: u32,
}

#[trame::state(steps(report))]
struct St {
    filled: u64,
}

#[trame::step(St)]
fn report(filled: &u64) -> Status {
    let step_none = u32::from(trame::system::left().is_none());
    if trame::send(trame::Leader, Seen { exact: u32::from(*filled == SHARE), step_none }).is_err() {
        return Status::wait().room::<Seen>(trame::Leader);
    }
    Status::Done
}

fn main() {
    // SAFETY: nothing else in this process reads the environment before the launch reads this.
    unsafe { std::env::set_var("TRAME_AVAILABLE", "33554432") };
    let leader = trame::is_leader();
    let mut t = Topology::new();
    let w = t.worker::<St>();
    t.set(&w, move || {
        let share = trame::system::info().worker_bytes.expect("a builder reads its share");
        assert_eq!(share, SHARE, "32 MiB less a sixteenth, over one worker");
        // Take a quarter first, then spend exactly what is left on the last structure.
        let mut first = trame::Bounded::<u64>::new((share / 4 / 8) as usize);
        for i in 0..(share / 4 / 8) as u64 {
            first.push(i).expect("capacity is the length");
        }
        let left = trame::system::left().expect("left in a builder");
        assert_eq!(left, share - first.len() as u64 * 8, "left is the share less what the builder took");
        let mut second = trame::Bounded::<u64>::new((left / 8) as usize);
        for i in 0..(left / 8) as u64 {
            second.push(i).expect("left is what fits");
        }
        let filled = (first.len() + second.len()) as u64 * 8;
        core::mem::forget((first, second));
        St { filled }
    });
    t.route::<Seen>(w.id(), trame::Leader, 1);
    let run = t.launch().expect("the exact spend is inside the share");
    let mut seen = None;
    while seen.is_none() {
        match run.recv::<Seen>() {
            Some(s) => seen = Some(s.value),
            None => run.wait(),
        }
    }
    run.join();
    if leader {
        let s = seen.expect("a report");
        println!("exact: {}", s.exact == 1);
        println!("step: {}", if s.step_none == 1 { "none" } else { "some" });
    }
}

/// Every trame program installs trame's allocator wrapper (domain.rs).
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
