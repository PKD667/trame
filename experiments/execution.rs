// What `#[parallel]` costs on a CPU, measured instead of asserted.
//
//   cargo run -p trame --example execution --release
//
// No MPI, no peers, no NERVE: the workload is a bag of cells and a stream of hits this file
// makes up, and the only thing under test is the claim — that an `#[ordered]` `#[parallel]`
// function run through `invoke!` is the loop a hand-written scalar reference would have been,
// with no added dispatch, no per-hit allocation and no queue.
//
// Two paths run the same work, and must agree on the final state *and* on the checksum of every
// write, in order:
//
//   reference  a hand-written loop with the update inlined into it.
//   invoked    the same update as a `#[parallel]` function keyed on the cell, through `invoke!`.
//
// On timing, honestly: this is a single process on a loaded developer machine and the two hot
// loops differ by less than the run-to-run spread. The number to read is the *ratio*, reported
// with the spread across rounds, and the correct conclusion from `1.00 ± 0.03` is "no measurable
// added cost", never "faster".

use std::convert::Infallible;
use std::time::Instant;

use trame::Keyed;

/// One unit of work. A payload, not a spike: this crate has no idea what a spike is.
#[derive(Clone, Copy)]
struct Hit {
    cell: usize,
    amount: f64,
}

/// The state the workload mutates: each cell's charge and the checksum of its writes in order.
struct Cells(Vec<(f64, u64)>);

impl Cells {
    fn new(n: usize) -> Self {
        Cells(vec![(0.0, 0); n])
    }

    /// Cells do not share a write order, so their checksums are summed.
    fn marks(&self) -> u64 {
        self.0.iter().fold(0, |sum, cell| sum.wrapping_add(cell.1))
    }
}

/// The update itself, shared by both paths so that a difference between them can only come from
/// the lowering and never from the arithmetic.
#[inline(always)]
fn apply((charge, marks): &mut (f64, u64), hit: Hit) {
    *charge = *charge * 0.97 + hit.amount;
    if *charge > 1.0 {
        *charge -= 1.0;
        // An order-sensitive checksum: it folds the cell index and the running total, so two
        // schedules that touch one cell in different orders disagree here.
        *marks = marks
            .rotate_left(7)
            .wrapping_add(hit.cell as u64)
            .wrapping_mul(0x9e3779b97f4a7c15);
    }
}

fn reference(cells: &mut Cells, hits: &[Hit]) {
    for &hit in hits {
        apply(&mut cells.0[hit.cell], hit);
    }
}

struct Pump;

impl Pump {
    #[trame::parallel]
    #[trame::ordered(key = hit.cell: usize)]
    fn charge(&self, hit: Hit, cell: &mut (f64, u64), _: &()) -> Result<(), Infallible> {
        apply(cell, hit);
        Ok(())
    }

    fn invoked(&self, cells: &mut Cells, hits: &[Hit]) {
        trame::invoke!(self.charge, &(), hits, Keyed::new(&mut cells.0[..]))
            .expect("every hit names a cell");
    }
}

/// A deterministic hit stream. A tiny xorshift, so two builds see the same workload.
fn workload(hits: usize, cells: usize, seed: u64) -> Vec<Hit> {
    let mut x = seed | 1;
    (0..hits)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            Hit {
                cell: (x as usize) % cells,
                amount: ((x >> 11) as f64 / (1u64 << 53) as f64) * 0.4,
            }
        })
        .collect()
}

/// Seconds, as the host measures them.
fn timed(mut body: impl FnMut()) -> f64 {
    let at = Instant::now();
    body();
    at.elapsed().as_secs_f64()
}

fn arg(name: &str, fallback: usize) -> usize {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("{name} takes a number"));
        }
    }
    fallback
}

fn main() {
    let cells = arg("--cells", 4096);
    let count = arg("--hits", 1 << 20);
    let rounds = arg("--rounds", 5);
    let hits = workload(count, cells, 0x5eed);
    let pump = Pump;

    // Correctness first: a benchmark of two things that do not agree measures nothing.
    let mut a = Cells::new(cells);
    reference(&mut a, &hits);
    let mut b = Cells::new(cells);
    pump.invoked(&mut b, &hits);
    assert_eq!(a.0, b.0, "the invoked path reached a different state or wrote in a different order");

    // And the timings, one line per round, in the order they ran.
    println!(
        "{{\"what\":\"config\",\"cells\":{cells},\"hits\":{count},\"rounds\":{rounds},\
         \"profile\":\"{}\",\"backend\":{}}}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        trame::ID.wire_id()
    );
    let mut ratios = Vec::new();
    for round in 0..rounds {
        let mut one = Cells::new(cells);
        let plain = timed(|| reference(&mut one, &hits));
        let mut two = Cells::new(cells);
        let invoked = timed(|| pump.invoked(&mut two, &hits));
        assert_eq!(one.0, two.0);
        ratios.push(invoked / plain);
        println!(
            "{{\"what\":\"round\",\"round\":{round},\"reference_s\":{plain:.6},\
             \"invoked_s\":{invoked:.6},\"ratio\":{:.4},\"checksum\":{}}}",
            invoked / plain,
            one.marks()
        );
    }
    ratios.sort_by(f64::total_cmp);
    let middle = ratios[ratios.len() / 2];
    let spread = ratios[ratios.len() - 1] - ratios[0];
    println!(
        "{{\"what\":\"summary\",\"ratio_median\":{middle:.4},\"ratio_spread\":{spread:.4},\
         \"verdict\":\"{}\"}}",
        if (middle - 1.0).abs() <= spread.max(0.02) {
            "invoked path within run-to-run spread of the hand-written reference"
        } else if middle > 1.0 {
            "invoked path measurably slower here: read the assembly before believing either way"
        } else {
            "invoked path measured faster, which at this sample size means the spread is the \
             measurement: read the assembly"
        }
    );
}
