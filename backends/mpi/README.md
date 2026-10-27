# trame MPI transport (one node)

Implements spec.md's Launch and Run sections for workers that share one node. Not wired into the
root `lib.rs`. Cross-node is refused (`Refusal::NotOneNode`) and untested. Results: `RESULT.md`.

## Ranks

With W workers, `mpirun` starts W + 1 ranks: ranks `0..W` are workers and rank W is the leader. The
leader is a separate rank because the publication comes from host code, not from a worker, and giving
that job to one worker would make it special at the barrier. This keeps every worker symmetric: the
leader runs no worker code and is never counted at a barrier.

## Mechanism

- One `MPI_Win_allocate_shared` window on the node communicator. The leader owns it; every route,
  counter, barrier word and publication lives in it. No MPI call after launch except the launch
  barrier. The window is never freed (`MPI_Win_free` is collective and would wait for peers).
- Each route has its own ring of `cap` slots, sized from that route's declared message size
  (`layout.rs`), not from `MAX`. Window bytes per route: 128 + cap × (align8(size) + 8).
- Lossless: the sender owns P, the receiver owns C. `Full` is decided by an acquire load of C.
  That load is the linearization point: `Full` exactly when `P − C == cap`.
- Lossy: a seqlock per slot. The sender never reads C and never waits. A receiver that sees a
  slot change under its copy skips the frame (a Drop).
- Receive: the head of the first owned route, in rotating order, that has a frame. The scan starts
  past the route the last receive took, so a route with frames is taken within n receives (n = owned
  routes). Frames are ordered within a route only, as spec.md's Offers definition says.
- Barrier: rootless. Each worker stores its generation in its arrive word and adds 1 to COUNT. The
  add that makes COUNT a multiple of W releases the generation: that worker adds 2 to GEN and wakes
  the waiters. Waiters sleep on GEN with `FUTEX_WAIT` (no PRIVATE flag, so it works across the
  processes that map the window). A worker that returned sets bit 0 of GEN and wakes the waiters,
  so a worker that never arrives is reported as `Deadlock` to every waiter.
- Publication: the leader's `publish` writes a pending deposit and waits until the previous one is
  applied. The releaser of the next barrier to complete copies the pending deposit into the
  publication every worker reads. The deposit therefore takes effect at a barrier, not when it is
  made, and each worker sees each publication once and in order.

## Tests (run inside `nix develop .#mpi`)

    backends/mpi/tests/run.sh          # all checks; exit 0 if all pass
    mpirun --oversubscribe -n 4 target/mpi/debug/check fifo --seed 1 --dir D    # 3 workers
    target/mpi/debug/check verify D
    mpirun --oversubscribe -n 3 target/mpi/debug/check overwrite --seed 7 --cap 4 --msgs 20000 [--late]
    mpirun --oversubscribe -n 4 target/mpi/debug/check fair --msgs 16
    mpirun --oversubscribe -n 6 target/mpi/debug/check barrier --seed 3 --rounds 300   # 5 workers
    mpirun --oversubscribe -n 5 target/mpi/debug/check deadlock                        # 4 workers
    mpirun --oversubscribe -n 3 target/mpi/release/latency --samples 6636 --dir D

`latency` writes raw round trips to `$D/records.jsonl` (nixsci record format). It computes no
verdict; an importer states its claim with nixsci.stats. Only release builds give timings, and a
record names the binary, toolchain, host, load and placement.

## Not checked

- Cross-node, other MPI implementations, other CPUs.
- Formal proof. The Full, Overwrite and futex sleep arguments are argued in the module comments
  and tested, not proven. The traced build adds SeqCst fences, so its checks exercise a more
  strongly ordered variant than production (see RESULT.md).
- Starvation beyond the bound above is not tested under time-varying load; `fair` tests the
  pre-filled case only.
- The spec's Publish rule fires while every worker waits. A deposit here waits for the next barrier
  instead, so the spec and this backend differ in when a publication changes. Spec wording to decide.
- The mpi-rma submodule is not used here; it allocates per frame, see RESULT.md.

## Finalization

A worker records its outcome in the window and wakes the leader before it calls MPI_Finalize; its
result is final by then, and a peer that is still waiting cannot change it. The leader calls
MPI_Finalize inside `join`, after it has read every outcome, and an `atexit` hook finalizes a rank
that exits without `join` (a refusal, a dropped run). A worker that finalizes early waits in
MPI_Finalize until the leader does. One unpinned measurement gave about 4% of one core while it
waits; it is not reproducible from this tree.
