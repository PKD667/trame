// The MPI-launched checks of the transport. N - 1 workers and the leader as rank N - 1; each
// subcommand is run by mpirun -n N. The leader runs no worker code.
//
//   mpirun -n N check fifo      --seed S --dir D      random routes and random interleavings;
//                                                     FIFO per route; every send and consume logged
//   mpirun -n N check overwrite --seed S --cap C --msgs M [--late]
//                                                     lossy route, 0 -> 1; newest frame kept
//   mpirun -n N check barrier   --seed S --rounds R   leader deposits each round; every worker sees
//                                                     the publications in order, the last one last
//   mpirun -n N check fair      --msgs M              two senders fill two routes to M frames; the
//                                                     receiver must alternate between them (no starvation)
//   mpirun -n N check deadlock                        worker 0 returns; the others must get Deadlock
//   check verify D                                    no MPI: checks the fifo logs in D
//
// A failed check panics, which makes mpirun stop the job with a nonzero status.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use mpi::traits::*;
use trame::__mpi::{clock, launch, Answer, Error, Event, Received, Role, Route, RouteId, Topology, Transport, MAX};

/// The worker transport of this rank, or None for the leader (the last rank).
fn worker(world: &mpi::topology::SimpleCommunicator, topo: &Topology) -> Option<Transport> {
    match launch(world, topo).unwrap_or_else(|e| panic!("refused: {e}")) {
        Role::Worker(t) => Some(t),
        Role::Leader(_) => None,
    }
}

/// A small seeded generator (xorshift64*). Each rank derives its own stream from the seed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// Busy work of a random length: this is how the interleavings vary, never a sleep.
    fn spin(&mut self, max: u64) {
        for _ in 0..self.below(max + 1) {
            std::hint::spin_loop();
        }
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn arg_num(args: &[String], name: &str, default: u64) -> u64 {
    arg(args, name).map_or(default, |v| v.parse().unwrap_or_else(|_| panic!("{name} wants a number")))
}

/// A random topology of `n` workers, from the seed alone, so every rank derives the same one.
fn random_topology(seed: u64, n: u32) -> Topology {
    let mut rng = Rng::new(seed ^ 0x51);
    let mut routes: Vec<Route> = Vec::new();
    for _ in 0..(3 * n + 2) {
        let from = rng.below(u64::from(n)) as u32;
        let to = rng.below(u64::from(n)) as u32;
        let tag = rng.below(3);
        if routes.iter().any(|r| (r.from, r.to, r.tag) == (from, to, tag)) {
            continue;
        }
        let size = [16, 200, MAX][rng.below(3) as usize];
        let cap = 1 + rng.below(8) as u32;
        routes.push(Route { from, to, tag, size, cap, lossy: false });
    }
    Topology { workers: n, routes, publication: b"init".to_vec() }
}

/// How many frames each route carries, from the seed and the route's rank only.
fn plan_counts(seed: u64, routes: usize, max: u64) -> Vec<u64> {
    (0..routes)
        .map(|r| Rng::new(seed ^ (r as u64).wrapping_mul(0x1000_0000_01b3) ^ 0xc0)
            .below(max + 1))
        .collect()
}

fn ids_to(routes: &[Route], me: u32) -> Vec<RouteId> {
    routes
        .iter()
        .enumerate()
        .filter(|(_, r)| r.to == me)
        .map(|(i, _)| RouteId(i as u32))
        .collect()
}

fn fill(route: u32, seq: u64, buf: &mut [u8]) {
    buf[0..4].copy_from_slice(&route.to_le_bytes());
    buf[4..12].copy_from_slice(&seq.to_le_bytes());
    for (k, b) in buf[12..].iter_mut().enumerate() {
        *b = (seq as u8).wrapping_add(k as u8);
    }
}

/// Check a received message's header and payload. Panics on any mismatch.
fn check_frame(route: u32, seq: u64, buf: &[u8]) {
    let got_route = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    let got_seq = u64::from_le_bytes(buf[4..12].try_into().unwrap());
    assert_eq!(got_route, route, "frame on route {route} carries route {got_route}");
    assert_eq!(got_seq, seq, "route {route}: FIFO broken, expected seq {seq}, got {got_seq}");
    for (k, b) in buf[12..].iter().enumerate() {
        assert_eq!(*b, (seq as u8).wrapping_add(k as u8), "route {route} seq {seq}: payload byte {k}");
    }
}

fn write_events(dir: &Path, me: u32, events: &[Event]) {
    let mut f = fs::File::create(dir.join(format!("rank-{me}.log"))).expect("create log");
    for e in events {
        match *e {
            Event::Send { route, index, read, before, after, answer } => writeln!(
                f,
                "S {route} {index} {read} {before} {after} {}",
                if answer == Answer::Ok { 0 } else { 1 }
            ),
            Event::Consume { route, index, before, after } => {
                writeln!(f, "C {route} {index} {before} {after}")
            }
        }
        .expect("write log");
    }
}

fn fifo(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let seed = arg_num(args, "--seed", 1);
    let max = arg_num(args, "--msgs", 300);
    let dir = PathBuf::from(arg(args, "--dir").expect("fifo wants --dir"));
    fs::create_dir_all(&dir).expect("log dir");
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    let topo = random_topology(seed, n);
    let Some(mut t) = worker(world, &topo) else { return };
    if me == 0 {
        let mut s = String::new();
        for (i, r) in topo.routes.iter().enumerate() {
            s += &format!("R {i} {} {} {} {} {}\n", r.from, r.to, r.size, r.cap, u8::from(r.lossy));
        }
        fs::write(dir.join("topo.txt"), s).expect("topo");
    }
    t.trace_on();

    let counts = plan_counts(seed, topo.routes.len(), max);
    let owned = ids_to(&topo.routes, me);
    let mut tokens: Vec<u32> = Vec::new();
    for (r, route) in topo.routes.iter().enumerate() {
        if route.from == me {
            tokens.extend(std::iter::repeat_n(r as u32, counts[r] as usize));
        }
    }
    let mut rng = Rng::new(seed ^ (u64::from(me) << 32) ^ 0xf1f0);
    for i in (1..tokens.len()).rev() {
        tokens.swap(i, rng.below(i as u64 + 1) as usize);
    }
    let expected_in: u64 = topo
        .routes
        .iter()
        .enumerate()
        .filter(|(_, r)| r.to == me)
        .map(|(i, _)| counts[i])
        .sum();

    let mut next = 0usize;
    let mut sent_seq = vec![0u64; topo.routes.len()];
    let mut recv_seq = vec![0u64; topo.routes.len()];
    let mut received = 0u64;
    let mut fulls = 0u64;
    let mut msg = vec![0u8; MAX];
    let mut buf = vec![0u8; MAX];
    while next < tokens.len() || received < expected_in {
        match rng.below(3) {
            0 if next < tokens.len() => {
                let r = tokens[next];
                let size = topo.routes[r as usize].size;
                fill(r, sent_seq[r as usize], &mut msg[..size]);
                match t.send(RouteId(r), &msg[..size]).expect("send") {
                    Answer::Ok => {
                        sent_seq[r as usize] += 1;
                        next += 1;
                    }
                    Answer::Full => fulls += 1,
                }
            }
            _ => {
                if let Received::Message(id) = t.recv(&owned, &mut buf).expect("recv") {
                    let r = id.0 as usize;
                    check_frame(id.0, recv_seq[r], &buf[..topo.routes[r].size]);
                    recv_seq[r] += 1;
                    received += 1;
                }
            }
        }
        rng.spin(40);
    }
    write_events(&dir, me, &t.take_trace());
    println!("fifo rank {me}: sent {} received {} full {fulls}", tokens.len(), received);
}

fn overwrite(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let seed = arg_num(args, "--seed", 1);
    let cap = arg_num(args, "--cap", 4) as u32;
    let msgs = arg_num(args, "--msgs", 5000);
    let late = args.iter().any(|a| a == "--late");
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    assert!(n >= 2, "overwrite wants two workers");
    let topo = Topology {
        workers: n,
        routes: vec![Route { from: 0, to: 1, tag: 0, size: 64, cap, lossy: true }],
        publication: Vec::new(),
    };
    let Some(mut t) = worker(world, &topo) else {
        // The leader joins the two harness barriers and nothing else.
        if late {
            world.barrier();
            world.barrier();
        }
        return;
    };
    let mut buf = vec![0u8; MAX];
    let r = RouteId(0);
    if me == 0 {
        let mut rng = Rng::new(seed);
        let mut msg = [0u8; 64];
        for i in 0..msgs {
            fill(0, i, &mut msg);
            let a = t.send(r, &msg).expect("send");
            assert_eq!(a, Answer::Ok, "lossy send answered Full at {i}");
            rng.spin(200);
        }
        println!("overwrite rank 0: sent {msgs} lossy frames, all Ok");
    }
    if late {
        // The harness, not the transport, makes the receiver wait for the sender to finish.
        world.barrier();
    }
    if me == 1 {
        let owned = [r];
        let last = msgs - 1;
        if late {
            // Every frame has been sent: the route must hold exactly the newest cap frames.
            let mut got = Vec::new();
            while let Received::Message(_) = t.recv(&owned, &mut buf).expect("recv") {
                got.push(u64::from_le_bytes(buf[4..12].try_into().unwrap()));
            }
            let want: Vec<u64> = (msgs.saturating_sub(u64::from(cap))..msgs).collect();
            assert_eq!(got, want, "late read: expected the newest {cap} frames");
            println!("overwrite rank 1 (late): got the newest {cap} frames {got:?}");
        } else {
            // Concurrent with the sender: the frames seen are increasing and the newest is kept.
            let mut prev: Option<u64> = None;
            let mut count = 0u64;
            let mut last_seen = None;
            while last_seen != Some(last) {
                if let Received::Message(_) = t.recv(&owned, &mut buf).expect("recv") {
                    let seq = u64::from_le_bytes(buf[4..12].try_into().unwrap());
                    if let Some(p) = prev {
                        assert!(seq > p, "lossy route reordered: {seq} after {p}");
                    }
                    prev = Some(seq);
                    last_seen = Some(seq);
                    count += 1;
                }
            }
            println!("overwrite rank 1: {count} of {msgs} frames seen, increasing, newest {last} kept");
        }
    }
    if late {
        world.barrier();
    }
}

fn barrier(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let seed = arg_num(args, "--seed", 1);
    let rounds = arg_num(args, "--rounds", 200);
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    let topo = Topology { workers: n, routes: Vec::new(), publication: b"init".to_vec() };
    let mut t = match launch(world, &topo).unwrap_or_else(|e| panic!("refused: {e}")) {
        Role::Worker(t) => t,
        Role::Leader(mut l) => {
            // One deposit per round. `publish` never waits, so later deposits replace earlier ones that
            // no barrier installed yet.
            for k in 1..=rounds {
                l.publish(format!("round {k}").as_bytes());
            }
            // The window lives in this rank: stay until every worker is done with it.
            world.barrier();
            return;
        }
    };
    let mut rng = Rng::new(seed ^ u64::from(me));
    let last = format!("round {rounds}").into_bytes();
    // Distinct values in the order this worker read them. The publication changes only at a
    // barrier, so a value is read at one barrier or more and never skipped.
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut calls = 0u64;
    // Keep meeting the others until the last deposit is read. A deposit is applied at a barrier
    // that completes after it, so the last one may need barriers after the leader has finished.
    while seen.last() != Some(&last) {
        calls += 1;
        assert!(calls <= 100 * rounds + 1000, "rank {me}: the last publication never arrived");
        rng.spin(2000);
        let got = t.barrier().expect("barrier").to_vec();
        if seen.last() != Some(&got) {
            seen.push(got);
        }
    }
    // Spec 5.5: a barrier answers the publication current when it fires, so a worker sees `init`
    // only if it reaches a barrier before the leader's first deposit is installed, and then rounds in
    // order, each at most once. The leader's deposits race the workers' barriers, so the exact
    // subsequence depends on timing; the order and the final value do not.
    let mut want_round = 0u64;
    for v in &seen {
        if v == b"init" {
            assert_eq!(want_round, 0, "rank {me}: init seen after a round");
            continue;
        }
        let k: u64 = std::str::from_utf8(v).expect("utf8").trim_start_matches("round ").parse().expect("round");
        assert!(k > want_round, "rank {me}: publication {k} out of order after {want_round}");
        want_round = k;
    }
    assert_eq!(want_round, rounds, "rank {me}: the last publication is not the final round");
    world.barrier();
    println!("barrier rank {me}: {rounds} rounds, publications seen in order, the last one last");
}

/// Workers 0 and 1 each send `msgs` frames to worker 2, on its own route, before the receiver
/// starts. Both routes then hold frames until the first one is drained, so a rotating receiver
/// must take them in turns: it may not take the same route twice in a row while both have frames.
fn fair(world: &mpi::topology::SimpleCommunicator, args: &[String]) {
    let msgs = arg_num(args, "--msgs", 16);
    let me = world.rank() as u32;
    assert!(world.size() == 4, "fair wants two senders, a receiver and the leader: four ranks");
    let route = |from| Route { from, to: 2, tag: 0, size: 64, cap: msgs as u32, lossy: false };
    let topo = Topology { workers: 3, routes: vec![route(0), route(1)], publication: Vec::new() };
    let Some(mut t) = worker(world, &topo) else {
        world.barrier();
        return;
    };
    let mut msg = [0u8; 64];
    if me < 2 {
        for i in 0..msgs {
            fill(me, i, &mut msg);
            assert_eq!(t.send(RouteId(me), &msg).expect("send"), Answer::Ok, "route {me} is not full");
        }
    }
    world.barrier();
    if me != 2 {
        return;
    }
    let owned = [RouteId(0), RouteId(1)];
    let mut buf = [0u8; 64];
    let mut order = Vec::new();
    for _ in 0..2 * msgs {
        match t.recv(&owned, &mut buf).expect("recv") {
            Received::Message(id) => order.push(id.0),
            Received::Empty => panic!("receiver found both routes empty before {} frames", 2 * msgs),
        }
    }
    for (i, pair) in order.windows(2).enumerate() {
        assert_ne!(pair[0], pair[1], "receives {i} and {} both took route {}", i + 1, pair[0]);
    }
    println!("fair rank 2: {msgs} frames per route, taken in turns: {order:?}");
}

fn deadlock(world: &mpi::topology::SimpleCommunicator) {
    let me = world.rank() as u32;
    let n = world.size() as u32 - 1;
    assert!(n >= 2, "deadlock wants two workers");
    let topo = Topology { workers: n, routes: Vec::new(), publication: Vec::new() };
    let Some(t) = worker(world, &topo) else { return };
    if me == 0 {
        // Return while the others wait at the barrier. Dropping the transport must not wait.
        println!("deadlock rank 0: returned at {}", clock());
        drop(t);
        return;
    }
    if me == 1 {
        // Keep working for two seconds first, so the barrier is still waited on when rank 0 leaves.
        let until = clock() + 2_000_000_000;
        while clock() < until {
            std::hint::spin_loop();
        }
    }
    let mut t = t;
    match t.barrier() {
        Err(Error::Deadlock) => println!("deadlock rank {me}: Deadlock at {}", clock()),
        other => panic!("rank {me}: expected Deadlock, got {other:?}"),
    }
}

fn verify(dir: &Path) {
    let topo = fs::read_to_string(dir.join("topo.txt")).expect("topo.txt");
    let mut routes: Vec<(u32, u32, u32, u64, bool)> = Vec::new(); // (from, to, cap, size, lossy)
    for line in topo.lines() {
        let f: Vec<u64> = line.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
        // f = [id, from, to, size, cap, lossy]
        routes.push((f[1] as u32, f[2] as u32, f[4] as u32, f[3], f[5] == 1));
    }
    let nroutes = routes.len();
    let mut sends: Vec<Vec<(u64, u64, u64, u64, bool)>> = vec![Vec::new(); nroutes]; // index, read, before, after, ok
    let mut consumes: Vec<Vec<(u64, u64, u64)>> = vec![Vec::new(); nroutes]; // index, before, after
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("log dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("rank-")))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no logs");
    for path in &files {
        for line in fs::read_to_string(path).expect("log").lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| -> u64 { f[i].parse().expect("number") };
            match f[0] {
                "S" => {
                    let r = num(1) as usize;
                    sends[r].push((num(2), num(3), num(4), num(5), num(6) == 0));
                }
                "C" => {
                    let r = num(1) as usize;
                    consumes[r].push((num(2), num(3), num(4)));
                }
                other => panic!("unknown log tag {other}"),
            }
        }
    }
    let mut total_sent = 0u64;
    let mut total_full = 0u64;
    for r in 0..nroutes {
        let (_, _, cap, _, lossy) = routes[r];
        if lossy {
            continue;
        }
        let cons = &mut consumes[r];
        cons.sort_by_key(|c| c.0);
        for (i, c) in cons.iter().enumerate() {
            assert_eq!(c.0, i as u64, "route {r}: consume {i} is frame {}", c.0);
            assert!(c.1 <= c.2, "route {r}: consume {i} ends before it starts");
        }
        // Sorted timestamps, for "how many consumes completed before t" by binary search.
        let mut after_sorted: Vec<u64> = cons.iter().map(|c| c.2).collect();
        let mut before_sorted: Vec<u64> = cons.iter().map(|c| c.1).collect();
        after_sorted.sort_unstable();
        before_sorted.sort_unstable();
        let completed_before = |t: u64| after_sorted.partition_point(|&a| a < t) as u64;
        let started_before = |t: u64| before_sorted.partition_point(|&b| b < t) as u64;

        let mut oks: Vec<u64> = Vec::new();
        for &(index, read, before, after, ok) in &sends[r] {
            let lo = completed_before(before);
            let hi = started_before(after);
            assert!(
                lo <= read && read <= hi,
                "route {r}: send {index} read C={read}, but {lo} consumes had completed and at most {hi} had started"
            );
            if ok {
                assert!(read <= index && index - read < u64::from(cap),
                    "route {r}: Ok send {index} with C={read}, cap {cap}");
                oks.push(index);
            } else {
                assert_eq!(index - read, u64::from(cap),
                    "route {r}: spurious Full: P={index} C={read} cap={cap}");
                total_full += 1;
            }
        }
        oks.sort_unstable();
        for (i, p) in oks.iter().enumerate() {
            assert_eq!(*p, i as u64, "route {r}: Ok sends are not P = 0,1,2,...");
        }
        assert_eq!(cons.len(), oks.len(), "route {r}: {} Ok sends but {} consumes", oks.len(), cons.len());
        total_sent += oks.len() as u64;
    }
    println!(
        "verify {}: {} logs, {} routes, {total_sent} frames sent and consumed in FIFO order, {total_full} Full answers, each with cap outstanding",
        dir.display(),
        files.len(),
        nroutes
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).cloned().unwrap_or_default();
    if cmd == "verify" {
        verify(Path::new(args.get(2).expect("verify wants a directory")));
        return;
    }
    let universe = mpi::initialize().expect("MPI");
    let world = universe.world();
    match cmd.as_str() {
        "fifo" => fifo(&world, &args),
        "overwrite" => overwrite(&world, &args),
        "barrier" => barrier(&world, &args),
        "fair" => fair(&world, &args),
        "deadlock" => deadlock(&world),
        other => panic!("unknown check {other:?}"),
    }
    // Finalization: the universe is dropped here, after this rank's work. The deadlock check
    // times it, to see that finalization does not wait for the rank still working.
    let me = world.rank();
    drop(world);
    drop(universe);
    if cmd == "deadlock" && me == 0 {
        println!("deadlock rank 0: finalized at {}", clock());
    }
}
