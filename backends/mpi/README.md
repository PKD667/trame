# trame MPI transport

The MPI backend runs a launched topology on one or more nodes (spec.md, Sections 4 and 5). Multi-node is the design
in `NODES.md`: each node has its own window and a relay thread pair, and needs MPI_THREAD_MULTIPLE
(without it, a multi-node launch is refused with `Refusal::Threads`). Wired into `programs/run.sh`,
which runs each program on one node and on the `TRAME_MPI_NODE_RANKS=k` splits (k = 1, 2). Results:
`RESULT.md`.

## Ranks

With W workers, `mpirun` starts W + 1 ranks: ranks `0..W` are workers and rank W is the leader. The
leader is a separate rank because the leader's calls (`send`, `recv`, `stop`, `join`) come from host code, not
from a worker, and giving that job to one worker would make it special. This keeps every worker symmetric:
the leader runs no worker code.

## Mechanism

- One `MPI_Win_allocate_shared` window per node, on the node communicator. The root node's leader
  owns the root window. Every route, counter and result word lives in a node's window.
  Outside the relay threads, no MPI call after launch except the launch barrier. The window is never freed (`MPI_Win_free` is collective and would wait for peers).
- Each route has its own ring of `cap` slots, sized from that route's declared message size
  (`layout.rs`), not from a frame limit. Window bytes per route: 128 + cap × (align8(size) + 8).
- Lossless: the sender owns P, the receiver owns C. `Full` is decided by an acquire load of C.
  That load is the linearization point: `Full` exactly when `P − C == cap`.
- Lossy: a seqlock per slot. The sender never reads C and never waits. A receiver that sees a
  slot change under its copy skips the frame (a Drop).
- Receive: the head of the first owned route, in rotating order, that has a frame. The scan starts
  past the route the last receive took, so a route with frames is taken within n receives (n = owned
  routes). Frames are ordered within a route only, as spec.md's Offers definition says.
- Wake-up. Each worker has an epoch word (`epoch.rs`). Every act that can change what one of its agents
  waits for bumps it, and an agent that must wait sleeps on it. A worker's result is its exit.
- States, the pool, edits and `take` live in the node's segment (`segment.rs`).

## Testing on one machine

`TRAME_MPI_NODE_RANKS=k` cuts each shared-memory node into groups of k ranks, each with its own window
and relay, so the multi-node code runs on one machine. The design is in `NODES.md` (Testing on one machine).

## Tests (run inside `nix develop .#mpi`)

    backends/mpi/tests/run.sh          # all checks, exit 0 if all pass
    mpirun --oversubscribe -n 4 target/mpi/debug/check fifo --seed 1 --dir D    # 3 workers
    target/mpi/debug/check verify D
    mpirun --oversubscribe -n 3 target/mpi/debug/check overwrite --seed 7 --cap 4 --msgs 20000 [--late]
    mpirun --oversubscribe -n 4 target/mpi/debug/check fair --msgs 16
    mpirun --oversubscribe -n 3 target/mpi/debug/check bigroute                        # 2 workers
    mpirun --oversubscribe -n 3 target/mpi/release/latency --samples 6636 --dir D
    TRAME_MPI_NODE_RANKS=2 mpirun --oversubscribe -n 4 target/mpi/debug/check fifo --seed 1 --dir D

The multi-node runs are in `run.sh`: the same checks with `TRAME_MPI_NODE_RANKS` set to 1 and to 2.
A failing check is named at the end, and the script exits non-zero.

`latency` writes raw round trips to `$D/records.jsonl` (nixsci record format). It computes no
verdict: an importer states its claim with nixsci.stats. Only release builds give timings, and a
record names the binary, toolchain, host, load and placement.

## Not checked

- Real multi-node hardware. Only the `TRAME_MPI_NODE_RANKS` splits on one machine are tested, not a real
  network. Other MPI implementations and other CPUs are not tested.
- Formal proof of the code. The futex wait, the credit of one route and the abort are model-checked
  (`proofs/`), on models that assume SeqCst and leave out timeouts. The Overwrite argument is argued in
  the module comments and tested, not modelled. The traced build adds SeqCst fences, so its checks exercise a more
  strongly ordered variant than production (see RESULT.md).
- Starvation beyond the bound above is not tested under time-varying load. `fair` tests the
  pre-filled case only.

## Finalization

A worker records its outcome in the window and wakes the leader before it calls MPI_Finalize. Its
result is final by then, and a peer that is still waiting cannot change it. The leader calls
MPI_Finalize inside `join`, after it has read every outcome, and an `atexit` hook finalizes a rank
that exits without `join` (a refusal, a dropped run). A worker that finalizes early waits in
MPI_Finalize until the leader does.
