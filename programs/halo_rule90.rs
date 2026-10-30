//! halo_rule90: rule 90 (new cell = left xor right) on a ring of 7 blocks of 16 cells, one block per
//! worker, from one live cell at cell 0, for 119 steps. Each step a worker sends its two edge cells to its
//! two neighbours (routes of capacity 1) and waits for theirs, so a worker that runs ahead is held back by
//! a full route. The light cone of 119 cells wraps the ring of 112, so the link between the last and the
//! first worker carries data. Each worker reports its block at the end; the leader prints them and the
//! number of live cells. The expected output comes from an independent simulation (halo_rule90.expected).

use trame::{Context, LEADER, Outcome, Status, Topology, WorkerId};

const WORKERS: usize = 7;
const BLOCK: usize = 16;
const STEPS: u32 = 119;

/// An edge cell sent to the left neighbour: its right halo.
#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct ToLeft {
    step: u32,
    cell: u32,
}

/// An edge cell sent to the right neighbour: its left halo.
#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct ToRight {
    step: u32,
    cell: u32,
}

#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Final {
    worker: u64,
    bits: u64,
}

struct Block {
    me: u64,
    left: WorkerId,
    right: WorkerId,
    cells: [u8; BLOCK],
    step: u32,
    sent_left: bool,
    sent_right: bool,
    halo_left: Option<u8>,
    halo_right: Option<u8>,
}

#[trame::process(receives = ToLeft, ToRight)]
impl Block {
    fn step(&mut self) -> Status {
        if self.step == STEPS {
            let bits = (0..BLOCK).fold(0u64, |b, j| b | (self.cells[j] as u64) << j);
            return match trame::send(LEADER, Final { worker: self.me, bits }) {
                Ok(()) => Status::Done,
                Err(_) => Status::wait().room::<Final>(LEADER),
            };
        }
        if !self.sent_left {
            if trame::send(self.left, ToLeft { step: self.step, cell: self.cells[0] as u32 }).is_err() {
                return Status::wait().room::<ToLeft>(self.left);
            }
            self.sent_left = true;
        }
        if !self.sent_right {
            if trame::send(self.right, ToRight { step: self.step, cell: self.cells[BLOCK - 1] as u32 }).is_err() {
                return Status::wait().room::<ToRight>(self.right);
            }
            self.sent_right = true;
        }
        // A ToRight comes from the left neighbour, a ToLeft from the right one.
        if self.halo_left.is_none() {
            if let Some(m) = trame::recv::<ToRight>() {
                assert_eq!(m.step, self.step, "a halo of another step");
                self.halo_left = Some(m.cell as u8);
            }
        }
        if self.halo_right.is_none() {
            if let Some(m) = trame::recv::<ToLeft>() {
                assert_eq!(m.step, self.step, "a halo of another step");
                self.halo_right = Some(m.cell as u8);
            }
        }
        let (Some(hl), Some(hr)) = (self.halo_left, self.halo_right) else {
            return Status::wait().frame();
        };
        let old = self.cells;
        for j in 0..BLOCK {
            let l = if j == 0 { hl } else { old[j - 1] };
            let r = if j == BLOCK - 1 { hr } else { old[j + 1] };
            self.cells[j] = l ^ r;
        }
        self.step += 1;
        (self.sent_left, self.sent_right, self.halo_left, self.halo_right) = (false, false, None, None);
        Status::Progress
    }
}

fn worker(cx: &Context) {
    let ws = cx.workers();
    let me = ws.iter().position(|&w| w == cx.me()).expect("a worker is listed");
    let mut cells = [0u8; BLOCK];
    if me == 0 {
        cells[0] = 1;
    }
    let block = Block {
        me: me as u64,
        left: ws[(me + WORKERS - 1) % WORKERS],
        right: ws[(me + 1) % WORKERS],
        cells,
        step: 0,
        sent_left: false,
        sent_right: false,
        halo_left: None,
        halo_right: None,
    };
    assert_eq!(trame::concurrent!(block;), Ok(()));
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<WorkerId> = (0..WORKERS).map(|_| t.worker(worker)).collect();
    for i in 0..WORKERS {
        t.route::<ToLeft>(ws[i], ws[(i + WORKERS - 1) % WORKERS], 1);
        t.route::<ToRight>(ws[i], ws[(i + 1) % WORKERS], 1);
        t.route::<Final>(ws[i], LEADER, 1);
    }
    let run = match t.launch() {
        Ok(run) => run,
        Err(e) => {
            println!("refused: {e:?}");
            return;
        }
    };
    let mut blocks: Vec<Option<u64>> = vec![None; WORKERS];
    while blocks.iter().any(Option::is_none) {
        match run.recv::<Final>() {
            Some(f) => blocks[f.worker as usize] = Some(f.bits),
            None => run.wait(),
        }
    }
    let mut ones = 0;
    for (i, b) in blocks.iter().enumerate() {
        let b = b.expect("every worker reported");
        ones += b.count_ones();
        println!("worker {i} block 0x{b:04x}");
    }
    println!("ones {ones}");
    let outcomes: Vec<Outcome> = run.join();
    println!("outcomes: {outcomes:?}");
}
