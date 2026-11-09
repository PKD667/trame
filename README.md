# Trame

A scientific program already has a communication graph. Nobody writes it down in one place.

Rank 3 sends halo cells to rank 4 and waits for a reply. Rank 4's buffer is full, so rank 3 blocks,
or the message is lost, or the queue grows until the node runs out of memory. On 1000 ranks one
rank is always late, and the run ends when the last one says so. Every one of these facts sits in a
different call.

Trame writes the graph down. A program names its workers, its messages and the capacity of each
route, and it cannot send outside them. The compiler checks the code, the launch fixes the graph, and
the backend must not produce a behaviour the spec forbids (CONTRACT.md).

## Example

`examples/ring.rs`, quoted whole (`tests/readme.rs` fails if the two differ). Four workers pass a token around a ring for three rounds, and
each reports its total to the leader. Worker `i` adds the values it takes from worker `i - 1`.

```rust
//! ring: W workers pass a token around a ring for ROUNDS rounds, then report to the leader.
use trame::{Leader, Status, Topology, Worker, WorkerId};

const W: usize = 4;
const ROUNDS: u32 = 3;

#[trame::message]
struct Token {
    value: u32,
}

#[trame::message]
struct Report {
    worker: u32,
    total: u32,
}

#[trame::state(steps(hop))]
struct Hop {
    me: u32,
    next: WorkerId,
    round: u32,
    value: u32,
    total: u32,
    sent: bool,
}

#[trame::step(Hop, receives = Token)]
fn hop(me: &u32, next: &WorkerId, round: &mut u32, value: &mut u32, total: &mut u32, sent: &mut bool) -> Status {
    if *round == ROUNDS {
        return match trame::send(Leader, Report { worker: *me, total: *total }) {
            Ok(()) => Status::Done,
            Err(_) => Status::wait().room::<Report>(Leader),
        };
    }
    if !*sent {
        if trame::send(*next, Token { value: *value }).is_err() {
            return Status::wait().room::<Token>(*next);
        }
        *sent = true;
    }
    match trame::recv::<Token>() {
        Some(t) => {
            (*value, *total, *round, *sent) = (t.value, *total + t.value, *round + 1, false);
            Status::Progress
        }
        None => Status::wait().frame(),
    }
}

fn main() {
    let mut topo = Topology::new();
    let ws: Vec<Worker<Hop>> = (0..W).map(|_| topo.worker()).collect();
    for i in 0..W {
        let next = ws[(i + 1) % W].id();
        topo.set(&ws[i], move || Hop { me: i as u32, next, round: 0, value: i as u32 + 1, total: 0, sent: false });
        topo.route::<Token>(ws[i].id(), next, 1);
        topo.route::<Report>(ws[i].id(), Leader, 1);
    }
    let run = topo.launch().expect("the ring launches");
    let mut totals = [None; W];
    while totals.iter().any(Option::is_none) {
        match run.recv::<Report>() {
            Some(r) => totals[r.worker as usize] = Some(r.total),
            None => run.wait(),
        }
    }
    for (i, t) in totals.iter().enumerate() {
        println!("worker {i} total {}", t.expect("every worker reported"));
    }
    println!("outcomes: {:?}", run.join());
}

/// Every trame program installs trame's allocator wrapper (domain.rs): a launch refuses otherwise.
#[global_allocator]
static ALLOC: trame::Alloc = trame::Alloc::new(std::alloc::System);
```

The output is the same on every backend, and on one host and split across hosts (`src/backends/README.md`,
Build and run). The four runs below are identical:

```
worker 0 total 9
worker 1 total 8
worker 2 total 7
worker 3 total 6
outcomes: [Returned, Returned, Returned, Returned]
```

The example does not handle a failed run. If a worker panics, the leader's next call panics too, with
`trame: worker <w> panicked: <message>` (INTERFACE.md 5, Abort). `check/programs/abort.rs` shows the leader in
`Run::wait`, and `check/programs/abort_join.rs` the leader in `join`.

## Build and run

The default build uses the reference backend:

    nix develop
    RUSTFLAGS='--cfg trame_checked' cargo run --example ring

The other backends, and the commands that run them, are in `src/backends/README.md` (Build and run).

The cfg flag is needed because `#[trame::state]` and `parallel!` are compile errors without it. The driver
sets it when it checks a build (INTERFACE.md, Section 3). Set by hand, it only lifts the error: it does not
stop a deliberate unchecked build.

A backend that runs workers as processes needs a rank count that matches the topology. The count for each
backend, and what it does with another one, is in `src/backends/README.md`.

## The levels

A program has three levels, each inside the one above it (INTERFACE.md, Section 1). Each one may do less.

**Host, leader.** Any Rust. It declares the topology, launches it, and drives the run through `Run`:
`send`, `recv`, `wait`, `edit`, `stop` and `join`. Nothing is checked here; it is the environment
(INTERFACE.md, Sections 4 and 5.5).

**Step.** A function over the fields of a worker's state, marked `#[trame::step]`. It may `send`,
`recv`, `me`, `now` and `parallel!`, and use counted loops. It may not allocate, recurse, use an
uncounted loop or block: it returns a `Status` (INTERFACE.md, Sections 3 and 5.3).

**Item.** A function given to `parallel!`. It may do arithmetic and call the listed core functions.
It may not perform an effect, allocate, recurse or use an uncounted loop (INTERFACE.md, Sections 3
and 5.4).

## Build and run: `trame`

    nix run .#trame -- build [BACKEND] [--check] [cargo args]     # e.g. --example ping_pong
    nix run .#trame -- run   [BACKEND] [-n RANKS] [--node-ranks K] PROGRAM [-- args]

`BACKEND` is the flag of a backend, empty for the reference one; `src/backends/README.md` (Build and run) lists
the flags. The backend is chosen at build time, so the flag goes to both. `--check` builds with the compile-time
checker (`cargo trame`, on the driver's own pinned rustc). A backend that places ranks asks the program how many
it needs, and the details are in `src/backends/README.md` (Build and run and Launch).

## What is checked, and how

| check | what it covers | explained in |
|---|---|---|
| the contract | the rules every other check enforces, and what a backend must not do | `INTERFACE.md`, `CONTRACT.md` |
| the macros | `#[trame::message]` refuses references, pointers, function pointers and padding; `#[trame::step]` takes fields and the pool as `&` or `&mut`; `#[trame::state]` requires `trame::Plain` of every field and the pool; `#[trame::state]` and `parallel!` need the cfg flag | `crates/macros/lib.rs` |
| `cargo trame check` | the rustc driver judges the MIR of each level: effects, counted loops, recursion, allocation | `crates/driver/README.md` |
| rejection suite | cases the checks must refuse, and their matched accepted cases | `check/rejections/README.md` |
| conformance by mutation | every rule has a program that fails on a backend which breaks it | `check/mutants/README.md` |
| scheduler model | `sched.rs` unit and property tests; trace replay of random workloads | `PROOFS.md` |
| crate tests | `cargo test` in the reference build | `tests/` |
| programs | every program in `check/programs/` on every backend, stdout compared with `.expected` | `check/programs/run.sh` |
| backend checks | each backend's own checks, such as the transport of a multi-process backend | `src/backends/README.md`, Checks by backend |
| TLA+ models | the wake-up, credit and abort protocols of some backends | `check/proofs/README.md` |
| `nix flake check` | builds the verdicts that must hold | `lab/README.md`, Verdicts |
| lab experiments | `nix run .#<name>` runs real processes: the experiments of each backend, and the scale runs | `lab/README.md`, `src/backends/README.md` |

`PROOFS.md` says, for each clause of `CONTRACT.md`, which of these checks it and where the check
stops.

## Repository map

- `docs/`: the system design, one file per part. `docs/overview.md` is the entry.
- `src/backends/`: one directory per backend. `src/backends/README.md` describes all of them, and it is the only
  place outside the code that names a backend.
- `check/mutants/`: mutants of each backend, and the runner that checks the programs kill them.
- `crates/driver/`: the rustc driver that judges MIR, and `cargo trame`.
- `lab/`: the lab runners and the claims they test.
- `crates/macros/`: the proc macros `message`, `state`, `step`, `item` and `parallel`.
- `src/math/`: `trame::math::f32` and `trame::math::f64`, numeric functions with the same bits on every backend.
- `check/programs/`: shared programs with their expected output, run by `check/programs/run.sh`.
- `check/proofs/`: the TLA+ models of some backends' protocols.
- `check/rejections/`: cases the compile-time checks must refuse or accept.
- `lab/bench/`: scaling benchmarks and their baselines (`src/backends/README.md`, Benchmarks).
- `tests/`: crate tests for allocation, `parallel!`, conformance, and this README's example.
- `examples/`: the example in this README.
- Top level: `INTERFACE.md` (the behaviour), `CONTRACT.md` (what a backend keeps), `PROOFS.md` (how that is
  checked), `lib.rs` and the modules it names (the surface and the core), `flake.nix` (pinned inputs,
  builds, lab experiments). `target/` is build output.

## Limits

- Backends: one per build (`src/backends/README.md`).
- The driver fuzzer is not a verdict: run it as `crates/driver/fuzz/README.md` says.
- Every backend in the tree reports `Deadlock` for a worker it shows stuck, and panics a leader whose
  `wait` can never return. A backend may hold the detection back, and `src/backends/README.md` (Limits) says
  when. A backend that shows less is allowed (`CONTRACT.md`, E2).
- The crate does not check the byte order of the target. Only the latency test does.
- A state has at most 63 fields, and a worker at most 64 steps, the stop step included. A step
  receives at most 16 message types, and a `.room` wait names one of a worker's first 64 routes.
  Exceeding one is refused or aborts with a message naming it. These are implementation limits, not
  part of the contract.
- No latency bound is set yet. The latency experiment measures every backend, and `lab/q.nix` holds
  the bounds it will be judged against (CONTRACT.md, Q); they stay empty until measured.
