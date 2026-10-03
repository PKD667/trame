// Distributed Jacobi relaxation with a serial reference.
// A square grid is split into row bands. Each step exchanges the top and bottom halo rows.
// Jacobi reads only the previous iterate, so the distributed result is bit-identical to `stencil`
// unless transport loses, reorders, or corrupts a frame.
//
//     mpirun -n 4 verifiable [grid] [steps]
//
// The dependency chain also stresses message rate and bandwidth.

#[path = "wire.rs"]
mod wire;
use wire::*;

/// Row sent to the next rank up. It is the receiver's top halo.
const DOWN: Tag = 0;
/// Row sent to the previous rank. It is the receiver's bottom halo.
const UP: Tag = 1;
/// Finished band sent to rank 0.
const BAND: Tag = 2;

/// Relax one cell as the mean of its four neighbors. `g` is row-major with width `n`.
fn stencil(g: &[f64], n: usize, i: usize, j: usize) -> f64 {
    0.25 * (g[(i - 1) * n + j] + g[(i + 1) * n + j] + g[i * n + j - 1] + g[i * n + j + 1])
}

/// Seed a cell from its global position.
fn seed(i: usize, j: usize) -> f64 {
    ((i * 31 + j * 17) % 97) as f64 / 97.0
}

/// Global rows `[lo, hi)` owned by rank `r`. Boundary rows are not updated.
fn band(r: Rank, n: usize, ranks: usize) -> (usize, usize) {
    let r = r as usize;
    (r * n / ranks, (r + 1) * n / ranks)
}

fn encode(row: &[f64]) -> Vec<u8> {
    row.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn decode(bytes: &[u8], into: &mut [f64]) {
    for (cell, chunk) in into.iter_mut().zip(bytes.chunks_exact(8)) {
        *cell = f64::from_le_bytes(chunk.try_into().unwrap());
    }
}

/// Relax the whole grid in one process for the reference result.
fn serial(n: usize, steps: usize) -> Vec<f64> {
    let mut cur: Vec<f64> = (0..n * n).map(|k| seed(k / n, k % n)).collect();
    let mut next = cur.clone();
    for _ in 0..steps {
        next.copy_from_slice(&cur);
        for i in 1..n - 1 {
            for j in 1..n - 1 {
                next[i * n + j] = stencil(&cur, n, i, j);
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    cur
}

fn main() {
    let mut wire = Wire::start();
    let (me, ranks) = (wire.rank(), wire.size() as usize);

    let mut args = std::env::args().skip(1);
    let n: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(512);
    let steps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(200);
    if n < ranks || n < 3 {
        if me == 0 {
            eprintln!("verifiable needs a grid at least {ranks} wide, got {n}");
        }
        wire.done();
        return;
    }

    let (lo, hi) = band(me, n, ranks);
    let rows = hi - lo;
    let (above, below) = (me > 0, (me as usize) < ranks - 1);

    // Allocate one halo row at each end. Missing halos remain unused.
    let mut cur = vec![0.0; (rows + 2) * n];
    for k in 1..=rows {
        for j in 0..n {
            cur[k * n + j] = seed(lo + k - 1, j);
        }
    }
    let mut next = cur.clone();

    let started = std::time::Instant::now();
    let mut bytes = 0u64;
    for _ in 0..steps {
        // Send both halos before receiving. Blocking sends can deadlock when halos exceed eager
        // capacity. The step dependency keeps ranks within one step of their neighbors.
        if above {
            wire.post(me - 1, UP, &encode(&cur[n..2 * n]));
            bytes += (n * 8) as u64;
        }
        if below {
            wire.post(me + 1, DOWN, &encode(&cur[rows * n..(rows + 1) * n]));
            bytes += (n * 8) as u64;
        }
        // One sender uses each tag. MPI pair ordering selects the current step's halo.
        if above {
            let (_, row) = wire.take(DOWN);
            decode(&row, &mut cur[0..n]);
        }
        if below {
            let (_, row) = wire.take(UP);
            decode(&row, &mut cur[(rows + 1) * n..(rows + 2) * n]);
        }

        next.copy_from_slice(&cur);
        for k in 1..=rows {
            let i = lo + k - 1;
            if i == 0 || i == n - 1 {
                continue;
            }
            for j in 1..n - 1 {
                next[k * n + j] = stencil(&cur, n, k, j);
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    let elapsed = started.elapsed().as_secs_f64();

    let mine = &cur[n..(rows + 1) * n];
    if me != 0 {
        wire.put(0, BAND, &encode(mine));
        wire.done();
        return;
    }

    let mut whole = vec![0.0; n * n];
    whole[lo * n..hi * n].copy_from_slice(mine);
    for _ in 1..ranks {
        let (from, payload) = wire.take(BAND);
        let (lo, hi) = band(from, n, ranks);
        decode(&payload, &mut whole[lo * n..hi * n]);
    }

    let truth = serial(n, steps);
    let worst = whole
        .iter()
        .zip(&truth)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f64, f64::max);

    println!("grid {n}x{n}, {steps} steps, {ranks} ranks");
    println!(
        "exchange {:.1} MB in {elapsed:.3} s",
        bytes as f64 / (1 << 20) as f64
    );
    assert_eq!(
        worst, 0.0,
        "distributed and serial differ; worst absolute error is {worst:e}"
    );
    println!("exact: distributed and serial agree to the bit");
    wire.done();
}
