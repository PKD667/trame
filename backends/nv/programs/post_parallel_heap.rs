//! post_parallel_heap: the program of post_parallel_pod, except that the worker keeps its slots in a
//! `Vec` it allocates itself. Same fold, same output. The point is what a plan can see: this worker
//! captures nothing (its closure is zero-sized), and the heap is made inside its body, so no check over
//! the launch plan can tell it from post_parallel_pod. Expected output: post_parallel_pod.expected.

use trame::{Context, LEADER, Outcome, Topology};

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    acc: u64,
    slot: u32,
    branch: u32,
}

/// One input: the slot it folds into, and its value.
struct Input {
    key: u32,
    value: u64,
}

/// The shared context of the items.
struct Ctx {
    salt: u64,
}

/// A slot. No `Copy`, and no heap: an accumulator and two counters.
struct Slot {
    acc: u64,
    seen: [u32; 2],
}

/// The fold is order-dependent (acc is multiplied before each add), so a lane that folds a key's
/// items out of input order changes `acc`.
#[trame::item(key = x.key as usize)]
fn fold(x: &Input, slot: &mut Slot, cx: &Ctx) {
    slot.acc = (slot.acc * 3 + x.value + cx.salt) % 1_000_003;
    slot.seen[(x.value % 2) as usize] += 1;
}

fn worker(_cx: &Context) {
    let inputs: [Input; 8] = core::array::from_fn(|i| Input { key: (i % 4) as u32, value: 3 + 7 * i as u64 });
    let cx = Ctx { salt: 5 };
    let mut slots: Vec<Slot> = (0..4).map(|_| Slot { acc: 0, seen: [0; 2] }).collect();
    trame::parallel!(fold, &cx, &inputs, &mut slots);
    // Everything below reads the slots as the items left them.
    for (k, s) in slots.iter().enumerate() {
        let branch = if s.seen[0] > s.seen[1] { 1 } else { 2 };
        let sent = trame::send(LEADER, Report { acc: s.acc, slot: k as u32, branch });
        assert!(sent.is_ok(), "a report did not fit");
    }
}

fn main() {
    let mut t = Topology::new();
    let w = t.worker(worker);
    t.route::<Report>(w, LEADER, 4);
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    for k in 0..4 {
        let r = loop {
            match run.recv::<Report>() {
                Some(r) => break r,
                None => run.wait(),
            }
        };
        println!("slot {} acc {} branch {}", r.slot, r.acc, r.branch);
        assert_eq!(r.slot, k);
    }
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}
