# Trame

A scientific program already has a communication graph. Nobody writes it down in one place.

Rank 3 sends halo cells to rank 4 and waits for a reply. Rank 4's buffer is full, so rank 3 blocks,
or the message is lost, or the queue grows until the node runs out of memory. On 1000 ranks one
rank is always late, and the run ends when the last one says so. Every one of these facts sits in a
different call.

Trame writes the graph down. A program names its workers, its messages and the capacity of each
route, and it cannot send outside them. The compiler checks the code, the launch fixes the graph, and
the backend must not produce a behaviour the spec forbids (spec.md, Section 6).

## Example

`examples/ring.rs`, included as it is. Four workers pass a token around a ring for three rounds, and
each reports its total to the leader. Worker `i` adds the values it takes from worker `i - 1`.

```rust
//! ring: W workers pass a token around a ring for ROUNDS rounds, then report to the leader.
use trame::{Context, Status, Topology, WorkerId, LEADER};

const W: usize = 4;
const ROUNDS: u32 = 3;

#[trame::message]
struct Token { value: u32 }

#[trame::message]
struct Report { worker: u32, total: u32 }

struct Hop { me: u32, next: WorkerId, round: u32, value: u32, total: u32, sent: bool }

#[trame::process(receives = Token)]
impl Hop {
    fn step(&mut self) -> Status {
        if self.round == ROUNDS {
            let report = Report { worker: self.me, total: self.total };
            return match trame::send(LEADER, report) { Ok(()) => Status::Done, Err(_) => Status::wait().room::<Report>(LEADER) };
        }
        if !self.sent {
            if trame::send(self.next, Token { value: self.value }).is_err() {
                return Status::wait().room::<Token>(self.next);
            }
            self.sent = true;
        }
        match trame::recv::<Token>() {
            Some(t) => {
                (self.value, self.total, self.round, self.sent) = (t.value, self.total + t.value, self.round + 1, false);
                Status::Progress
            }
            None => Status::wait().frame(),
        }
    }
}

fn worker(cx: &Context) {
    let me = cx.workers().iter().position(|&w| w == cx.me()).expect("a worker is listed");
    let hop = Hop { me: me as u32, next: cx.workers()[(me + 1) % W], round: 0, value: me as u32 + 1, total: 0, sent: false };
    assert_eq!(trame::concurrent!(hop;), Ok(()));
}

fn main() {
    let mut topo = Topology::new();
    let ws: Vec<WorkerId> = (0..W).map(|_| topo.worker(worker)).collect();
    for i in 0..W { topo.route::<Token>(ws[i], ws[(i + 1) % W], 1); topo.route::<Report>(ws[i], LEADER, 1); }
    let run = topo.launch().expect("the ring launches");
    let mut totals = vec![None; W];
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
```

Output, on seq and on MPI (five ranks), and on MPI with `TRAME_MPI_NODE_RANKS` set to 1 and to 2.
The four outputs are identical:

```
worker 0 total 9
worker 1 total 8
worker 2 total 7
worker 3 total 6
outcomes: [Returned, Returned, Returned, Returned]
```

The example does not handle a failed run. If a worker panics, the leader waits forever: `Run::wait`
returns on an abort, but it does not say why, so the loop goes on waiting. `programs/abort.rs` shows the leader that calls `join` and
reads `Outcome::Abort`.

## Build and run

Seq, the default backend and the reference:

    nix develop
    RUSTFLAGS='--cfg trame_checked' cargo run --example ring

MPI:

    nix develop .#mpi
    RUSTFLAGS='--cfg trame_checked' CARGO_TARGET_DIR=target/mpi \
      cargo build --no-default-features --features mpi --example ring
    mpirun --oversubscribe -n 5 target/mpi/debug/examples/ring

The cfg flag is needed because `concurrent!` and `parallel!` are compile errors without it. The driver
sets it when it checks a build (spec.md, Section 3); by hand, it only lifts the error, and it does not
stop a deliberate unchecked build.

`mpirun` needs W + 1 ranks: one per worker, and the leader on the last rank. With four workers, that
is five. Any other count is refused on every rank before a worker starts. Open MPI may print
`osc_ucx_component` errors on stderr on some machines; they come from Open MPI, not from the program.

## The levels

A program has four levels, each inside the one above it (spec.md, Section 1). Each one may do less.

| level | where it runs | may | may not | spec |
|---|---|---|---|---|
| host, leader | any Rust | build the topology, launch, `send`, `recv`, `wait`, `publish`, `join` through `Run` | nothing is checked here; it is the environment | 4, 5.2, 5.5 |
| worker | a worker's `run` | any loop, recursion and allocation; `send`, `recv`, `now`, `barrier`, `concurrent!`, `parallel!` | the operating system, `unsafe`, `std`, threads, globals | 1, 3 |
| step | `step` of a process in `concurrent!` | `send`, `recv`, `now`, `parallel!`; counted loops; allocation | recursion, uncounted loops, blocking: it returns a `Status` | 1, 3, 5.3 |
| item | a function given to `parallel!` | arithmetic and listed core functions | every effect, allocation, recursion, uncounted loops | 1, 3, 5.4 |

## What is checked, and how

| check | what it covers | explained in |
|---|---|---|
| the contract | the rules every other check enforces, and what a backend must not do | `spec.md` |
| the macros | `#[trame::message]` refuses references, pointers, function pointers and padding; `concurrent!` and `parallel!` need the cfg flag | `macros/lib.rs` |
| `cargo trame check` | the rustc driver judges the MIR of each level: effects, counted loops, recursion, allocation | `driver/README.md` |
| rejection suite | cases the checks must refuse, and their matched accepted cases | `rejections/README.md` |
| conformance by mutation | every rule has a program that fails on a backend which breaks it | `conformance/README.md` |
| crate tests | `cargo test` in the seq build; `sched.rs` enumerates scheduler states up to a bound (a bounded check, not a proof) | `sched.rs`, `tests/` |
| programs | every program in `programs/` on seq and MPI, stdout compared with `.expected` | `programs/run.sh` |
| MPI checks | the transport: FIFO, overwrite, barrier, deadlock, on one node and split across nodes | `backends/mpi/tests/run.sh` |
| `nix flake check` | the Nix checks the rejection suite and the driver fuzzer (see Status) | `experiments/README.md` |
| lab experiments | `nix run .#<name>`: `programs-seq`, `programs-mpi`, `transport-mpi`, `scale-local`, `mutants-seq`, `mutants-mpi` | `experiments/README.md` |

## Repository map

- `backends/`: `seq`, the reference; `mpi`, MPI ranks, memory windows and relays. `nv` holds a plan
  (`PLAN.md`), not a backend.
- `conformance/`: mutants of each backend, and the runner that checks the programs kill them.
- `driver/`: the rustc driver that judges MIR, and `cargo trame`.
- `experiments/`: the lab runners and the claims they test.
- `macros/`: the proc macros `message`, `process`, `item`, `concurrent`, `parallel`.
- `programs/`: shared programs with their expected output, run by `programs/run.sh`.
- `rejections/`: cases the compile-time checks must refuse or accept.
- `scale/`: scaling benchmarks and their plain-MPI baselines.
- `tests/`: crate tests for the allocation and `parallel!` rules.
- `examples/`: the example in this README.
- Top level: `spec.md` (the contract), `lib.rs` and the modules it names (the surface and the core),
  `flake.nix` (pinned inputs, builds, lab experiments). `target/` is build output.

## Status and limits

- Backends: seq and mpi. The MPI backend runs several nodes only on one machine, through
  `TRAME_MPI_NODE_RANKS`. It has not run on a real network (`backends/mpi/README.md`, Not checked).
- `nix flake check`: `experiments/README.md` calls the rejection suite and the fuzzer Nix checks, but
  `flake.nix` has no `checks` output at this commit. Run them as their READMEs say.
- Not built: detecting a stuck run after `join` on MPI. Seq does it (`backends/seq/ops.rs`).
- Not built: a leader that can tell an abort from a change in `Run::wait`. The example above hangs for
  that reason.
- Open point: when a publication made during a barrier takes effect on MPI (`backends/mpi/README.md`,
  Not checked).
- The crate does not check the byte order of the target. Only the latency test does.
- Implementation limits, not part of the contract: the scheduler keeps its conditions in 64-bit
  masks, so one `concurrent!` holds at most 64 processes and 64 cells, and a `.room` wait names
  one of a worker's first 64 routes. Exceeding one aborts with a message naming it.
- Latency: the MPI backend records round trips, but no nixsci experiment locks them yet, so no latency
  claim is checked (spec.md, Section 6).
- The CUDA backend, its device heap, warp lowering and `cargo nv` are on the `legacy` branch, for
  reference only.
