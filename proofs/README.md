# Proofs

Model checks of the MPI backend's protocols, in TLA+ (TLC 2.19, tlaplus 1.7.4, from the flake's pinned nixpkgs).
A model is an abstraction. A passing check says the abstraction has the property at the bounds shown, not that
the code has it.

## Models (`mpi/`)

- **Epoch** (`Epoch.tla`, `MCEpoch.cfg`). The wait protocol of one worker or the leader: `backends/mpi/epoch.rs` 40-119 and
  `futex.rs` 17-27. Properties: `NoLostWakeup`, `Live` (a true condition is eventually seen), under weak fairness.
  Bounds: epoch word modulo 16, 3 bumps. Holds, 109 states.
- **Epoch wrap** (`MCEpochWrap.cfg`). Same model at modulus 2, 2 bumps. Expected to fail `NoLostWakeup`. The trace
  is `counterexamples/epoch-wrap.txt`, step by step against the code. A miss needs a multiple of W bumps between
  the waiter's read and the kernel's compare. Also run at W=3, K=2 (holds) and W=3, K=3 (fails).
- **Credit** (`Credit.tla`, `MCCredit.cfg`). One lossless route, capacity 2, 4 frames, from `relay_out.rs` and
  `relay_routes.rs`, with `NODES.md` "A route between nodes". Properties: occupancy at most C, no over-send, FIFO
  order, every frame eventually credited. Holds, 168 states. Also holds at C=1 N=3 and C=3 N=5.
- **Stale Full** (`MCCreditFull.cfg`). Expected to fail: the sender reports Full while every frame has arrived
  and the receiver has room, because a consumption has not been credited yet. This state is allowed (spec 5.2).
  Trace in `counterexamples/credit-stale-full.txt`.
- **Abort** (`Abort.tla`, `MCAbort.cfg`). Abort and end over three nodes, from `NODES.md` "Abort" and "End" and
  `leader.rs` `leader_panicked`. `Reach` and `Live` hold: an abort reaches every node, every rank ends and every
  relay stops. This model found a bug, since repaired: `leader_panicked` returned without setting END when an
  abort had arrived before the leader's own panic, and no relay stopped. `MCAbortEarly.cfg` keeps that early
  return and must violate `Live`. The trace is `counterexamples/abort-leader-panic.txt`.

## Not modelled

Memory orderings weaker than SeqCst (`epoch.rs` 113 is Relaxed), timeouts of `sleep_on`, the kernel beyond
compare-and-sleep, MPI's progress beyond weak fairness, two routes per node (so no scan that sends frames and
credits of different routes), lossy routes, process exit and `MPI_Finalize`, the leader's normal end.

## Run

`sh proofs/mpi/run.sh OUTDIR` with `TLC` set to a tlc binary, or `nix build .#checks.x86_64-linux.protocol-mpi`.
The check runs all six MC configs and writes `outcome.json`, `results.txt` and the logs. Numbers of distinct
states for failing runs vary between runs (TLC stops at the first violation with 4 workers).
