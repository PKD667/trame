# Proofs

Model checks of the MPI backend's protocols, in TLA+ (TLC 2.19, tlaplus 1.7.4, from the flake's pinned nixpkgs).
A model is an abstraction. A passing check says the abstraction has the property at the bounds shown, not that
the code has it.

## Models (`mpi/`)

- **Epoch** (`Epoch.tla`, `MCEpoch*.cfg`). The wait protocol of one worker rank or the leader: `backends/mpi/epoch.rs`
  (`sleep_on`, `bump_word`) and `futex.rs` `wait_for`, `wake`. N sleepers (the k agents of a worker rank, `worker.rs`)
  share one epoch word and one SLEEP word; one bumper acts K times. SLEEP is a count: `fetch_add` before the futex wait,
  `fetch_sub` after it, and a bumper wakes when it loads a non-zero count (`COUNTED = TRUE`). `COUNTED = FALSE` is the
  earlier flag (store 1, store 0). `LOCKED = TRUE` adds `XPORT` (`worker.rs` 32), held from `idle` until `sleep_on`
  returns, so one agent of a rank sleeps at a time; `LOCKED = FALSE` is the word's protocol alone, which is what
  `epoch.rs` argues and what k agents see if the lock is ever narrowed. Properties: `NoLostWakeup`, `Live` (every
  sleeper sees a true condition), under weak fairness; `Announced` (SLEEP is non-zero while any sleeper has added and
  not subtracted, the fact `bump_word` relies on); `CountExact` (SLEEP equals that number).
  Configurations, all W = 16, K = 3 bumps, all count; all hold (TLC 2.19, chiclet-5, 2026-11-02; distinct states,
  depth): `MCEpoch` N=1 (223, 21), `MCEpoch2` N=2 (3,171, 27), `MCEpoch3` N=3 (43,375, 34), `MCEpochXport` N=3 with
  `XPORT` (27,364, 26).
- **Flag** (`MCEpochFlag.cfg`, N=2, `COUNTED = FALSE`). Fails `NoLostWakeup` (a 26-state trace): agent A sleeps;
  agent B adds itself, and A's wake-up clears the flag while B still sleeps; the next bump loads 0 and does not wake
  B. `MCEpochFlagAnnounced.cfg` fails `Announced`, the cause, in 14 states. `MCEpochFlagXport.cfg` (flag, N=2, with
  `XPORT`) holds (2,577 distinct states, depth 24): under the lock only one agent of a rank sleeps at a time, and
  the flag loses no wake among them. So the hang the count fixed did not come from a rank's agents racing on the
  flag under `XPORT`; it came from a sleeper outside that lock (the leader's or the relay's `sleep_on`). Which one
  is not established.
  Trace: `counterexamples/epoch-flag-k2.txt`.
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

Memory orderings weaker than SeqCst, timeouts of `sleep_on`, spurious futex returns, a bumper on the same rank that needs `XPORT` while an agent sleeps holding it, the kernel beyond
compare-and-sleep, MPI's progress beyond weak fairness, two routes per node (so no scan that sends frames and
credits of different routes), lossy routes, process exit and `MPI_Finalize`, the leader's normal end.

## Run

`sh proofs/mpi/run.sh OUTDIR` with `TLC` set to a tlc binary, or `nix build .#checks.x86_64-linux.protocol-mpi`.
The check runs all the MC configs and writes `outcome.json`, `results.txt` and the logs. Numbers of distinct
states for failing runs vary between runs (TLC stops at the first violation with 4 workers).
