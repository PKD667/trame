// Particle migration simulation independent of NERVE.
// A square grid assigns one cell to each rank. Particles collide locally and migrate across cells.
// `trame` is the only project dependency.
//
//     mpirun -n 4 particles [steps] [per_rank]
//
// The rank count must be a perfect square. Output reports particles kept, migrations, and
// collisions per rank. The final count checks conservation across the wire.

#[path = "wire.rs"]
mod wire;
use wire::*;

/// Particles that have left their cell, packed one after another.
const MIGRATE: Tag = 0;
/// End of a rank's migrations. Carries nothing.
const SETTLED: Tag = 1;
/// A rank's four summary counts, on the way to rank 0.
const REPORT: Tag = 2;

/// Side of the box. Cells are `SIDE / grid` across.
const SIDE: f64 = 1.0;
/// How close two particles must be to count as colliding.
const RADIUS: f64 = 0.004;
/// Distance per step at unit speed.
const DT: f64 = 0.002;
/// Bytes one particle takes on the wire.
const WIDTH: usize = 32;

/// Four little-endian 8-byte values used for particles and summaries.
/// A fixed width lets a received lane be decoded by chunks.
fn quad(bytes: &[u8]) -> [[u8; 8]; 4] {
    std::array::from_fn(|i| bytes[i * 8..i * 8 + 8].try_into().unwrap())
}

fn pack(vals: [[u8; 8]; 4]) -> Vec<u8> {
    vals.concat()
}

#[derive(Clone, Copy)]
struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
}

impl Particle {
    fn encode(&self, into: &mut Vec<u8>) {
        into.extend_from_slice(&pack(
            [self.x, self.y, self.vx, self.vy].map(f64::to_le_bytes),
        ));
    }

    fn decode(bytes: &[u8]) -> Self {
        let [x, y, vx, vy] = quad(bytes).map(f64::from_le_bytes);
        Particle { x, y, vx, vy }
    }
}

/// Deterministic per rank for reproducible runs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Which rank owns the cell containing `(x, y)`, on a `grid x grid` decomposition.
fn owner(x: f64, y: f64, grid: usize) -> Rank {
    let cell = |v: f64| ((v / SIDE * grid as f64) as usize).min(grid - 1);
    (cell(y) * grid + cell(x)) as Rank
}

fn main() {
    let mut wire = Wire::start();
    let (me, ranks) = (wire.rank(), wire.size() as usize);
    let grid = (ranks as f64).sqrt() as usize;
    if grid * grid != ranks {
        if me == 0 {
            eprintln!("particles needs a square rank count, got {ranks}");
        }
        wire.done();
        return;
    }

    let mut args = std::env::args().skip(1);
    let steps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(2000);
    let per_rank: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(500);

    // Seed inside each rank's cell.
    let (col, row) = (me as usize % grid, me as usize / grid);
    let cell = SIDE / grid as f64;
    let mut rng = Rng(0x9e3779b9 ^ me as u64);
    let mut mine: Vec<Particle> = (0..per_rank)
        .map(|_| Particle {
            x: (col as f64 + rng.next()) * cell,
            y: (row as f64 + rng.next()) * cell,
            vx: rng.next() - 0.5,
            vy: rng.next() - 0.5,
        })
        .collect();

    let peers: Vec<Rank> = (0..ranks as Rank).filter(|&r| r != me).collect();
    let (mut crossed, mut hits) = (0u64, 0u64);
    let mut outbound: Vec<Vec<u8>> = vec![Vec::new(); ranks];

    // Ranks can finish steps at different times. Drain every tag to avoid dropping an early frame.
    let mut reports: Vec<[u64; 4]> = Vec::new();
    let mut marks = 1;

    for _ in 0..steps {
        for p in &mut mine {
            p.x += p.vx * DT;
            p.y += p.vy * DT;
            // Reflect at the closed boundary. Particles leave cells but not the world.
            if p.x < 0.0 || p.x > SIDE {
                p.x = p.x.clamp(0.0, SIDE);
                p.vx = -p.vx;
            }
            if p.y < 0.0 || p.y > SIDE {
                p.y = p.y.clamp(0.0, SIDE);
                p.vy = -p.vy;
            }
        }

        // Collisions are local. Pairs across a cell boundary are not tested.
        for i in 0..mine.len() {
            for j in i + 1..mine.len() {
                let (dx, dy) = (mine[i].x - mine[j].x, mine[i].y - mine[j].y);
                if dx * dx + dy * dy < RADIUS * RADIUS {
                    let (left, right) = mine.split_at_mut(j);
                    std::mem::swap(&mut left[i].vx, &mut right[0].vx);
                    std::mem::swap(&mut left[i].vy, &mut right[0].vy);
                    hits += 1;
                }
            }
        }

        for lane in &mut outbound {
            lane.clear();
        }
        mine.retain(|p| {
            let to = owner(p.x, p.y, grid);
            if to == me {
                return true;
            }
            p.encode(&mut outbound[to as usize]);
            crossed += 1;
            false
        });
        for (to, lane) in outbound.iter().enumerate() {
            if !lane.is_empty() {
                wire.post(to as Rank, MIGRATE, lane);
            }
        }

        // Send all departures before draining. A late particle remains conserved.
        while drain(&mut wire, &mut mine, &mut reports, &mut marks) {}
    }

    // MPI preserves order between a pair. A settlement mark follows that peer's last migration.
    wire.broadcast(&peers, SETTLED, &[]);
    while marks < ranks {
        drain(&mut wire, &mut mine, &mut reports, &mut marks);
    }

    let summary = [me as u64, mine.len() as u64, crossed, hits];
    if me != 0 {
        wire.post(0, REPORT, &pack(summary.map(u64::to_le_bytes)));
        wire.done();
        return;
    }

    // After settlement, rank 0 receives only summaries.
    reports.push(summary);
    while reports.len() < ranks {
        drain(&mut wire, &mut mine, &mut reports, &mut marks);
    }
    reports.sort_unstable();

    let mut total = [0u64; 3];
    for [r, kept, crossed, hits] in reports {
        println!("rank {r}\tkept {kept}\tcrossed {crossed}\thits {hits}");
        for (sum, v) in total.iter_mut().zip([kept, crossed, hits]) {
            *sum += v;
        }
    }
    let [kept, crossed, hits] = total;
    let started = (ranks * per_rank) as u64;
    assert_eq!(kept, started, "particle count changed in transit");
    println!("total\tkept {kept}\tcrossed {crossed}\thits {hits}\tconserved of {started}");
    wire.done();
}

/// Drain one waiting frame and report whether one was found.
fn drain(
    wire: &mut Wire,
    mine: &mut Vec<Particle>,
    reports: &mut Vec<[u64; 4]>,
    marks: &mut usize,
) -> bool {
    match wire.poll() {
        Some((_, MIGRATE, data)) => mine.extend(data.chunks_exact(WIDTH).map(Particle::decode)),
        Some((_, SETTLED, _)) => *marks += 1,
        Some((_, REPORT, data)) => reports.push(quad(&data).map(u64::from_le_bytes)),
        Some((from, tag, _)) => eprintln!("rank {from} sent an unknown tag {tag}"),
        None => return false,
    }
    true
}
