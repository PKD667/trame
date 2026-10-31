//! fanin_fifo: six workers each send 3000 numbered items to the leader over a route of capacity 8, so a
//! worker waits for room whenever the leader is behind. Item `seq` of worker `i` carries seq*(i+1)+7. The
//! leader asserts that each sender's items arrive in order and exactly once (Section 5.2, FIFO and
//! credit), and prints count and sum per worker. The sums have a closed form: (i+1)*M(M-1)/2 + 7M.
//! Expected output: fanin_fifo.expected, from that formula.

use trame::{Context, LEADER, Outcome, Status, Topology};

const WORKERS: usize = 6;
const ITEMS: u32 = 3000;

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Item {
    seq: u32,
    from: u32,
    val: u64,
}

struct Feeder {
    from: u32,
    next: u32,
}

#[trame::process]
impl Feeder {
    fn step(&mut self) -> Status {
        // A step's loops are counted (spec.md, Definition (Counted loop)): a range from where the last step stopped.
        for seq in self.next..ITEMS {
            let val = seq as u64 * (self.from as u64 + 1) + 7;
            if trame::send(LEADER, Item { seq, from: self.from, val }).is_err() {
                return Status::wait().room::<Item>(LEADER);
            }
            self.next = seq + 1;
        }
        Status::Done
    }
}

fn worker(cx: &Context) {
    let me = cx.workers().iter().position(|&w| w == cx.me()).expect("a worker is listed") as u32;
    assert_eq!(trame::concurrent!(Feeder { from: me, next: 0 };), Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS).map(|_| t.worker(worker)).collect();
    for &w in &ws {
        t.route::<Item>(w, LEADER, 8);
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
