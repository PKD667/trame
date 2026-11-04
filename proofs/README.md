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
  flag under `XPORT`; it came from a sleeper outside that lock (the leader's `sleep_on`, or that of a relay
  thread this backend no longer has). Which one is not established.
  Trace: `counterexamples/epoch-flag-k2.txt`.
- **Lane** (`Lane.tla`, `MCLane.cfg`, `MCLane3.cfg`). One lossless route between hosts, from `backends/mpi/cross.rs`
  `Out` and `In`: the sender's room by credits, the receiver's posted receives, taking a frame, and the credit
  sent only when the last one completed. Properties: the chain c <= q <= got <= p <= c + C, every frame in flight
  has its receive posted (`Posted`), FIFO order, the sender never waits on a send that has not matched
  (`NoBlock`), one credit send outstanding, and every frame eventually taken and credited. Configurations
  C=2 N=5 (58 distinct states) and C=3 N=7 (177) hold (TLC 2.19, chiclet-6). `MCLaneOver.cfg` gives the sender
  one frame more room than its credits and must violate `Chain`; it does, once the sender has sent C + 1 frames on no credit.

## Not modelled

Memory orderings weaker than SeqCst, timeouts of `sleep_on`, spurious futex returns, a bumper on the same rank that needs `XPORT` while an agent sleeps holding it, the kernel beyond
compare-and-sleep, MPI's progress beyond weak fairness, several lanes at once, lossy lanes and their pending frame,
process exit, `quiesce` and `MPI_Finalize`, the leader's normal end, an abort that ends one host while a worker of
another waits on a lane.

## Run

`sh proofs/mpi/run.sh OUTDIR` with `TLC` set to a tlc binary, with `tlc` from nixpkgs' `tlaplus`.
The check runs all the MC configs and writes `outcome.json`, `results.txt` and the logs. Numbers of distinct
states for failing runs vary between runs (TLC stops at the first violation with 4 workers).
