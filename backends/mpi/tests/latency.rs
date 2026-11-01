// Round trips A -> B -> A between two ranks, recorded for nixsci. No verdict is computed here: the
// importer states its claim over the records (spec.md, Latency).
//
//   mpirun -n 3 latency [--samples N] [--max-gap-ns G] [--seed S] [--bytes B] [--dir D]
//
// Ranks 0 and 1 are the workers, rank 2 is the leader, which runs no worker code. `--bytes B` is the size
// of each probe, 8 to 65,536: the route's frame size. The default, 64, is the record kept in RESULT.md.
//
// The route label (`route_class`) says how the probe travelled, and is checked against the launch:
//   p2p / rma-lane   the path, from TRAME_MPI_SMALL (RMA.md); rma-lane only for a route of at most
//                    SMALL_BYTES between nodes, and the relay's lane count must equal the probes sent.
//   same-node / node-split-one-machine / cross-host
//                    the placement. A split by TRAME_MPI_NODE_RANKS on one machine is not cross-node: its
//                    two sides share the machine's memory, and the label says so.
//
// Output: one JSON line per round trip in $NIX_LAB_DIR/records.jsonl (or D when unset), in the
// record format of nixsci's lab/SPEC.md. `ns` is A's round trip on A's clock. Before each send
// A draws a target gap from [0, G] ns (xorshift64*, modulo bias below 1e-13 for G = 5e5), waits
// it out busy-waiting on the same clock, and records both the target (`gap_target_ns`) and the
// idle time actually spent (`gap_ns`), so an importer can check the spacing it was given.
//
// A probe is `seq` (u64, little-endian) and padding to `--bytes`. Both ends are one binary, and big-endian is
// refused.
use mpi::collective::SystemOperation;
use mpi::traits::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use trame::__mpi::{Answer, Received, Role, Route, RouteId, SMALL_BYTES, Topology, clock, launch, lane_frames};

#[path = "latency_util.rs"]
mod util;
use util::{Rng, arg, arg_num, utc_now};

#[cfg(target_endian = "big")]
compile_error!("the probe is sent as its bytes in native layout; this check needs a little-endian target");

/// The sequence number at the head of a probe.
fn seq_of(buf: &[u8]) -> u64 {
    u64::from_le_bytes(buf[0..8].try_into().expect("eight bytes"))
}

/// Where the two workers are, as the launch placed them. `same_node` follows nodes.rs: ranks 0 and 1 share a
/// node when no split is set, or when 0 / k == 1 / k. `one_machine` is whether every rank is on one host.
fn placement(split: Option<i32>, one_machine: bool) -> &'static str {
    let same_node = split.is_none_or(|k| 0 / k == 1 / k);
    if same_node {
        "same-node"
    } else if one_machine {
        "node-split-one-machine"
    } else {
        "cross-host"
    }
}

/// Whether every rank of `world` reports the same host name. A hash of the name, compared by min and max.
fn one_machine(world: &mpi::topology::SimpleCommunicator) -> bool {
    let name = mpi::environment::processor_name().expect("processor name");
    let h = name.bytes().fold(0x811c_9dc5u32, |a, b| (a ^ u32::from(b)).wrapping_mul(0x0100_0193)) as i32;
    let (mut lo, mut hi) = (0i32, 0i32);
    world.all_reduce_into(&h, &mut lo, SystemOperation::min());
    world.all_reduce_into(&h, &mut hi, SystemOperation::max());
    lo == hi
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let samples = arg_num(&args, "--samples", 6636);
    let max_gap = arg_num(&args, "--max-gap-ns", 500_000);
    let seed = arg_num(&args, "--seed", 1);
    let bytes = arg_num(&args, "--bytes", 64) as usize;
    assert!((8..=65_536).contains(&bytes), "--bytes wants 8 to 65536");
    let dir = arg(&args, "--dir").map(PathBuf::from).or_else(|| std::env::var_os("NIX_LAB_DIR").map(PathBuf::from));
    let run = std::env::var("NIX_LAB_RUN").unwrap_or_else(|_| "local".into());

    let (universe, _) = mpi::initialize_with_threading(mpi::Threading::Multiple).expect("MPI");
    let world = universe.world();
    let me = world.rank();
    assert!(world.size() == 3, "latency wants two workers and the leader: three ranks");
    let topo = Topology {
        workers: 2,
        routes: vec![
            Route { from: 0, to: 1, tag: 0, size: bytes, cap: 8, lossy: false },
            Route { from: 1, to: 0, tag: 0, size: bytes, cap: 8, lossy: false },
        ],
        publication: Vec::new(),
    };
    // The launch's placement and path, as the label says them (see the header).
    let split = std::env::var("TRAME_MPI_NODE_RANKS").ok().and_then(|v| v.parse::<i32>().ok());
    let path = std::env::var("TRAME_MPI_SMALL").unwrap_or_else(|_| "p2p".into());
    let place = placement(split, one_machine(&world));
    let lane = path == "rma" && place != "same-node" && bytes <= SMALL_BYTES;
    let route_class = format!("{}-{place}", if lane { "rma-lane" } else { "p2p" });
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
    let mut buf = vec![0u8; bytes];

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
            let mut p = vec![0u8; bytes];
            p[0..8].copy_from_slice(&i.to_le_bytes());
            while t.send(ab, &p).expect("send") != Answer::Ok {
                std::hint::spin_loop();
            }
            loop {
                match t.recv(&[ba], &mut buf).expect("recv") {
                    Received::Message(_) => break,
                    Received::Empty => std::hint::spin_loop(),
                }
            }
            let t1 = clock();
            assert_eq!(seq_of(&buf), i, "echo out of order");
            let ns = t1 - t0;
            rtts.push(ns);
            let line = format!(
                "{{\"id\":\"{run}-{i}\",\"time\":\"{}\",\"name\":\"rtt\",\"kind\":\"value\",\
                 \"data\":{{\"ns\":{ns},\"bytes\":{bytes},\"route_class\":\"{route_class}\",\
                 \"gap_ns\":{gap},\"gap_target_ns\":{target},\"max_gap_ns\":{max_gap},\"seq\":{i},\"seed\":{seed}}},\"tags\":{{}}}}\n",
                utc_now()
            );
            log.write_all(line.as_bytes()).expect("append record");
            idle_from = clock();
        }
        rtts.sort_unstable();
        let q = |f: f64| rtts[((rtts.len() - 1) as f64 * f).round() as usize];
        println!(
            "latency: n={} bytes={bytes} route={route_class} min={} median={} p99={} max={} ns (A's clock)",
            rtts.len(),
            q(0.0),
            q(0.5),
            q(0.99),
            q(1.0)
        );
        // The machine-readable line that latency_sizes.sh reads: no units, tab separated.
        println!("summary\t{bytes}\t{route_class}\t{}\t{}\t{}\t{}\t{}", rtts.len(), q(0.0), q(0.5), q(0.99), q(1.0));
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
    // The relay has stopped: its lane count is final. Every probe this rank's node sent went over the lane
    // when the label says so, and over no lane otherwise.
    let lanes = lane_frames();
    let expect = if lane { samples } else { 0 };
    assert_eq!(lanes, expect, "rank {me}: lane frames {lanes}, route class {route_class} wants {expect}");
    std::mem::forget(universe);
}
