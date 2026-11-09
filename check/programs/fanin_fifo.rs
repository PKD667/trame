//! fanin_fifo: six workers each send 3000 numbered items to the leader over a route of capacity 8, so a
//! worker waits for room whenever the leader is behind. Item `seq` of worker `i` carries seq*(i+1)+7. The
//! leader asserts that each sender's items arrive in order and exactly once (Section 5.2, FIFO and
//! credit), and prints count and sum per worker. The sums have a closed form: (i+1)*M(M-1)/2 + 7M.
//! Expected output: fanin_fifo.expected, from that formula.

use trame::{Leader, Outcome, Status, Topology};

const WORKERS: usize = 6;
const ITEMS: u32 = 3000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    seq: u32,
    from: u32,
    val: u64,
}

/// Worker `from`: the next item to send.
#[trame::state(steps(feed))]
struct Feeder {
    from: u32,
    next: u32,
}

/// Sends from `next` on until the route is full or every item is sent. The loop is counted
/// (INTERFACE.md, Definition (Counted loop)): a range from where the last call stopped.
#[trame::step(Feeder)]
fn feed(from: &u32, next: &mut u32) -> Status {
    for seq in *next..ITEMS {
        let val = seq as u64 * (*from as u64 + 1) + 7;
        if trame::send(Leader, Item { seq, from: *from, val }).is_err() {
            return Status::wait().room::<Item>(Leader);
        }
        *next = seq + 1;
    }
    Status::Done
}

fn main() {
    let mut t = Topology::new();
    for i in 0..WORKERS as u32 {
        let w = t.worker();
        t.set(&w, move || Feeder { from: i, next: 0 });
        t.route::<Item>(w.id(), Leader, 8);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let mut next = [0u32; WORKERS];
    let mut sum = [0u64; WORKERS];
    let mut got = 0;
    while got < WORKERS as u32 * ITEMS {
        match run.recv::<Item>() {
            Some(m) => {
                let w = m.from as usize;
                assert_eq!(m.seq, next[w], "worker {w}: an item out of order, lost or duplicated");
                next[w] += 1;
                sum[w] += m.val;
                got += 1;
            }
            None => run.wait(),
        }
    }
    for w in 0..WORKERS {
        println!("worker {w} count {} sum {}", next[w], sum[w]);
    }
    println!("total {}", sum.iter().sum::<u64>());
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
