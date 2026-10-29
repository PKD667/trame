// Round trips A -> B -> A between two ranks, recorded for nixsci. No verdict is computed here: the
// importer states its claim over the records (spec.md, Latency).
//
//   mpirun -n 3 latency [--samples N] [--max-gap-ns G] [--seed S] [--dir D]
//
// Ranks 0 and 1 are the workers, rank 2 is the leader, which runs no worker code.
//
// Output: one JSON line per round trip in $NIX_LAB_DIR/records.jsonl (or D when unset), in the
// record format of nixsci's lab/SPEC.md. `ns` is A's round trip on A's clock. Before each send
// A draws a target gap from [0, G] ns (xorshift64*, modulo bias below 1e-13 for G = 5e5), waits
// it out busy-waiting on the same clock, and records both the target (`gap_target_ns`) and the
// idle time actually spent (`gap_ns`), so an importer can check the spacing it was given.
//
// `Probe` is sent as its bytes in Rust's layout: both ends are one binary, and big-endian is refused.
use mpi::traits::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use trame::__mpi::{Answer, Received, Role, Route, RouteId, Topology, clock, launch};

#[path = "latency_util.rs"]
mod util;
use util::{Rng, arg, arg_num, utc_now};

#[cfg(target_endian = "big")]
compile_error!("the probe is sent as its bytes in native layout; this check needs a little-endian target");

#[repr(C)]
#[derive(Clone, Copy)]
struct Probe {
    seq: u64,
    payload: [u8; 56],
}

const BYTES: usize = std::mem::size_of::<Probe>();

fn probe_bytes(p: &Probe) -> &[u8] {
    unsafe { std::slice::from_raw_parts((p as *const Probe).cast::<u8>(), BYTES) }
}

fn probe_from(buf: &[u8]) -> Probe {
    assert_eq!(buf.len(), BYTES);
    unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<Probe>()) }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples = arg_num(&args, "--samples", 6636);
    let max_gap = arg_num(&args, "--max-gap-ns", 500_000);
    let seed = arg_num(&args, "--seed", 1);
    let dir = arg(&args, "--dir").map(PathBuf::from).or_else(|| std::env::var_os("NIX_LAB_DIR").map(PathBuf::from));
    let run = std::env::var("NIX_LAB_RUN").unwrap_or_else(|_| "local".into());

    let (universe, _) = mpi::initialize_with_threading(mpi::Threading::Multiple).expect("MPI");
    let world = universe.world();
    let me = world.rank();
    assert!(world.size() == 3, "latency wants two workers and the leader: three ranks");
    let topo = Topology {
        workers: 2,
        routes: vec![
            Route { from: 0, to: 1, tag: 0, size: BYTES, cap: 8, lossy: false },
            Route { from: 1, to: 0, tag: 0, size: BYTES, cap: 8, lossy: false },
        ],
        publication: Vec::new(),
    };
    let mut t = match launch(&world, &topo).unwrap_or_else(|e| panic!("refused: {e}")) {
        Role::Worker(t) => t,
        Role::Leader(l) => {
            trame::__mpi::leader_join(&l);
            trame::__mpi::finalize();
            std::mem::forget(universe);
            return;
        }
    };
    let (ab, ba) = (RouteId(0), RouteId(1));
    let mut buf = vec![0u8; BYTES];

    if me == 0 {
        let dir = dir.expect("rank 0 wants --dir or NIX_LAB_DIR");
        fs::create_dir_all(&dir).expect("lab dir");
        let mut log =
            OpenOptions::new().create(true).append(true).open(dir.join("records.jsonl")).expect("records.jsonl");
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut rtts = Vec::with_capacity(samples as usize);
        // `idle_from` is the end of the previous record's write, so logging is never counted as
        // spacing and the drawn gap is what separates two sends.
        let mut idle_from = clock();
        for i in 0..samples {
            // Randomized spacing, busy-waited on the same clock the round trip is timed on.
            let target = if max_gap == 0 { 0 } else { rng.next() % (max_gap + 1) };
            while clock() < idle_from + target {
                std::hint::spin_loop();
            }
            let t0 = clock();
            let gap = t0 - idle_from;
            let p = Probe { seq: i, payload: [0; 56] };
            while t.send(ab, probe_bytes(&p)).expect("send") != Answer::Ok {
                std::hint::spin_loop();
            }
            loop {
                match t.recv(&[ba], &mut buf).expect("recv") {
                    Received::Message(_) => break,
                    Received::Empty => std::hint::spin_loop(),
                }
            }
            let t1 = clock();
            assert_eq!(probe_from(&buf).seq, i, "echo out of order");
            let ns = t1 - t0;
            rtts.push(ns);
            let line = format!(
                "{{\"id\":\"{run}-{i}\",\"time\":\"{}\",\"name\":\"rtt\",\"kind\":\"value\",\
                 \"data\":{{\"ns\":{ns},\"bytes\":{BYTES},\"route_class\":\"shm-one-node\",\
                 \"gap_ns\":{gap},\"gap_target_ns\":{target},\"max_gap_ns\":{max_gap},\"seq\":{i},\"seed\":{seed}}},\"tags\":{{}}}}\n",
                utc_now()
            );
            log.write_all(line.as_bytes()).expect("append record");
            idle_from = clock();
        }
        rtts.sort_unstable();
        let q = |f: f64| rtts[((rtts.len() - 1) as f64 * f).round() as usize];
        println!(
            "latency: n={} bytes={BYTES} min={} median={} p99={} max={} ns (A's clock)",
            rtts.len(),
            q(0.0),
            q(0.5),
            q(0.99),
            q(1.0)
        );
    } else {
        let p_in = |t: &mut trame::__mpi::Transport, buf: &mut [u8]| loop {
            match t.recv(&[ab], buf).expect("recv") {
                Received::Message(_) => return,
                Received::Empty => std::hint::spin_loop(),
            }
        };
        for _ in 0..samples {
            p_in(&mut t, &mut buf);
            while t.send(ba, &buf).expect("send") != Answer::Ok {
                std::hint::spin_loop();
            }
        }
    }
    trame::__mpi::returned(&t);
    drop(t);
    trame::__mpi::finalize();
    std::mem::forget(universe);
}
