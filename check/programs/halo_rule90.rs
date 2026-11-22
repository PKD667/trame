//! halo_rule90: rule 90 (new cell = left xor right) on a ring of 7 blocks of 16 cells, one block per
//! worker, from one live cell at cell 0, for 119 steps. Each step a worker sends its two edge cells to its
//! two neighbours (routes of capacity 1) and waits for theirs, so a worker that runs ahead is held back by
//! a full route. The light cone of 119 cells wraps the ring of 112, so the link between the last and the
//! first worker carries data. Each worker reports its block at the end; the leader prints them and the
//! number of live cells. The expected output comes from an independent simulation (halo_rule90.expected).

use trame::{Leader, Outcome, Status, Topology, WorkerId};

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

/// One block of the ring, its two neighbours, and where the current step stands: which edges were sent
/// and which halos arrived.
#[trame::state(steps(advance))]
struct Block {
    me: u64,
    left: WorkerId,
    right: WorkerId,
    cells: [u8; BLOCK],
    step: u32,
    sent: [bool; 2],
    halo: [Option<u8>; 2],
}

/// One call does what it can of the current step: send the edges, take the halos, and once both are
/// in, compute the next cells. One step owns both halo types, since the next cells need both.
#[trame::step(Block, receives = ToLeft, ToRight)]
fn advance(
    me: &u64,
    left: &WorkerId,
    right: &WorkerId,
    cells: &mut [u8; BLOCK],
    step: &mut u32,
    sent: &mut [bool; 2],
    halo: &mut [Option<u8>; 2],
) -> Status {
    if *step == STEPS {
        let mut bits = 0u64;
        for j in 0..BLOCK {
            bits |= (cells[j] as u64) << j;
        }
        return match trame::send(Leader, Final { worker: *me, bits }) {
            Ok(()) => Status::Done,
            Err(_) => Status::wait().room::<Final>(Leader),
        };
    }
    if !sent[0] {
        if trame::send(*left, ToLeft { step: *step, cell: cells[0] as u32 }).is_err() {
            return Status::wait().room::<ToLeft>(*left);
        }
        sent[0] = true;
    }
    if !sent[1] {
        if trame::send(*right, ToRight { step: *step, cell: cells[BLOCK - 1] as u32 }).is_err() {
            return Status::wait().room::<ToRight>(*right);
        }
        sent[1] = true;
    }
    // A ToRight comes from the left neighbour, a ToLeft from the right one.
    if halo[0].is_none() {
        if let Some(m) = trame::recv::<ToRight>() {
            assert_eq!(m.step, *step, "a halo of another step");
            halo[0] = Some(m.cell as u8);
        }
    }
    if halo[1].is_none() {
        if let Some(m) = trame::recv::<ToLeft>() {
            assert_eq!(m.step, *step, "a halo of another step");
            halo[1] = Some(m.cell as u8);
        }
    }
    let [Some(hl), Some(hr)] = *halo else {
        return Status::wait().frame();
    };
    let old = *cells;
    for j in 0..BLOCK {
        let l = if j == 0 { hl } else { old[j - 1] };
        let r = if j == BLOCK - 1 { hr } else { old[j + 1] };
        cells[j] = l ^ r;
    }
    *step += 1;
    *sent = [false; 2];
    *halo = [None; 2];
    Status::Progress
}

fn main() {
    let mut t = Topology::new();
    let ws: Vec<_> = (0..WORKERS).map(|_| t.worker::<Block>()).collect();
    let ids: Vec<WorkerId> = ws.iter().map(|w| w.id()).collect();
    for (i, w) in ws.iter().enumerate() {
        let (left, right) = (ids[(i + WORKERS - 1) % WORKERS], ids[(i + 1) % WORKERS]);
        t.set(w, move || {
            let mut cells = [0u8; BLOCK];
            if i == 0 {
                cells[0] = 1;
            }
            Block { me: i as u64, left, right, cells, step: 0, sent: [false; 2], halo: [None; 2] }
        });
        t.route::<ToLeft>(ids[i], left, 1);
        t.route::<ToRight>(ids[i], right, 1);
        t.route::<Final>(ids[i], Leader, 1);
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
        match run.recv::<Final>().map(|s| s.value) {
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

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
