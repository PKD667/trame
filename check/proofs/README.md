# Proofs

Model checks of the MPI backend's protocols, in TLA+ (TLC 2.19, tlaplus 1.7.4, from the flake's pinned nixpkgs).
A model is an abstraction. A passing check says the abstraction has the property at the bounds shown, not that
the code has it.

## Models (`mpi/`)

- **Epoch** (`Epoch.tla`, `MCEpoch*.cfg`). The wait protocol of one worker rank or the leader: `src/backends/mpi/waiting/epoch.rs`
  (`sleep_on`, `bump_word`) and `waiting/futex.rs` `wait_for`, `wake`. N sleepers (the k agents of a worker rank, `roles/worker.rs`)
  share one epoch word and one SLEEP word; one bumper acts K times. SLEEP is a count: `fetch_add` before the futex wait,
  `fetch_sub` after it, and a bumper wakes when it loads a non-zero count (`COUNTED = TRUE`). `COUNTED = FALSE` is the
  earlier flag (store 1, store 0). `LOCKED = TRUE` adds `XPORT` (`roles/worker.rs` 39), held from `idle` until `sleep_on`
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
- **Lane** (`Lane.tla`, `MCLane.cfg`, `MCLane3.cfg`). One lossless route between hosts, from `src/backends/mpi/transport/cross/`
  `Out` and `In`: the sender's room by credits, the receiver's posted receives, taking a frame, and the credit
  sent only when the last one completed. Properties: the chain c <= q <= got <= p <= c + C, every frame in flight
  has its receive posted (`Posted`), FIFO order, the sender never waits on a send that has not matched
  (`NoBlock`), one credit send outstanding, and every frame eventually taken and credited. Configurations
  C=2 N=5 (58 distinct states) and C=3 N=7 (177) hold (TLC 2.19, chiclet-6). `MCLaneOver.cfg` gives the sender
  one frame more room than its credits and must violate `Chain`; it does, once the sender has sent C + 1 frames on no credit.
- **Detect** (`Detect.tla`, `MCDetect.cfg`, `MCDetectOnce.cfg`). The stuck detector: a Stuck verdict is given only when
  every worker is quiet or done and nothing is in flight. H = 2 hosts, STABLE = TRUE, BUDGET = 2, ROUNDS = 3 holds
  (7,351,279 distinct states, depth 49; TLC 2.19, MIST, 2026-11-16, in `run.sh`). Wider bounds also hold, run by hand
  with 2 workers and a 2.5 GB heap: BUDGET 2 ROUNDS 2 (2,215,512 states), BUDGET 3 ROUNDS 2 (24,660,766 states, 313 s).
  BUDGET 3 ROUNDS 3 did not finish in 900 s (39 M states, no error yet). `MCDetectOnce` is the mutant that skips the
  comparison with the previous round; it fails `Sound` (119,025 states), as it must.
- **Edit** (`Edit.tla`, `MCEdit*.cfg`). The handshake of a leader edit with a worker's steps (`worker.rs` `remote_edit`,
  `backends/mpi/roles/worker.rs` `edit_held`, `roles/leader.rs`): the worker holds its fields while the leader's closure runs.
  Two model bugs were found and fixed against the code while writing it (the leader's kernel entry of its sleep,
  and the Dekker re-read on the worker side). `MCEditNoWait` (`WAITREL = FALSE`, the leader does not wait for the
  release) fails `Held` (168,817 states), as it must; it is in `run.sh`. `MCEdit`, `MCEdit2`, `MCEdit22` and
  `MCEditXportAck` are larger models that did not finish: TLC found no error in 0.3 to 2.5 M distinct states before the
  timeout (1,200 to 30,854 s; `MCEdit` ran 8.5 hours). They check nothing yet beyond that prefix. Not modelled:
  the pool target, the `has_lanes` branch of `idle`, `fail_if_aborted`, the IDLE word.

## Not modelled

Memory orderings weaker than SeqCst, timeouts of `sleep_on`, spurious futex returns, a bumper on the same rank that needs `XPORT` while an agent sleeps holding it, the kernel beyond
compare-and-sleep, MPI's progress beyond weak fairness, several lanes at once, lossy lanes and their pending frame,
process exit, `quiesce` and `MPI_Finalize`, the leader's normal end, an abort that ends one host while a worker of
another waits on a lane.

## Run

`sh check/proofs/mpi/run.sh OUTDIR` with `TLC` set to a tlc binary, with `tlc` from nixpkgs' `tlaplus`.
The check runs all the MC configs and writes `outcome.json`, `results.txt` and the logs. Numbers of distinct
states for failing runs vary between runs (TLC stops at the first violation with 4 workers).

## Unsafe code and Miri

- `unsafe-registry.sh ROOT` lists every `unsafe` site in the Rust sources and fails on a site with no `SAFETY:` comment
  citing a clause of `CONTRACT.md`. It reads source only. It is a measure, not yet a gate: no check runs it, and many
  sites still fail it. `sh check/proofs/unsafe-registry.sh .` prints them.
**What runs.** `miri.sh` runs the seq backend's tests under Miri with Tree Borrows and isolation on: the library's unit tests except `math::`, `tests/conformance.rs`, `tests/parallel.rs` and `tests/alloc.rs`. The four suites run as separate jobs, each with its own target directory. The script exits non-zero if any job fails, and prints the names of the failed jobs.

**How to run.** From the repository root, `nix develop .#miri --command sh check/proofs/miri.sh`. Read the exit status directly: a pipe (`| tail`) reports the status of its last command, not of the script. Expected counts: lib 52 passed and 2 ignored, conformance 5 passed and 1 ignored, parallel 4, alloc 2. I ran the script with exit status 0 three times (no failing run was produced, so the non-zero path is not exercised).

**Cost.** 82 s and 87 s with the target directories warm, and 96 s on the first run, which builds the dependencies. Machine: 12 threads (`nproc`), AMD Ryzen 5 PRO 4650U; `cargo miri --version` gives `miri 0.1.0 (66da6cae1a 2026-04-20)`, rustc `1.97.0-nightly` (66da6cae1, 2026-04-20). Another job shared the machine during these runs, so the times are noisy by several seconds. The serial run before the split took 219 s on the same tree.

**What it found.** Commit f7fc1a5 fixes a data race, which Miri reported (per the commit message; I did not check which test reported it first). Before the fix, `State::run` took the pool as `&P`, so a step that did not take the pool still held a `&P` while the leader's edit wrote the pool through `&mut P`. CONTRACT.md P2 forbids that. Now `State::run` takes `*const P`, and only a step with the POOL bit makes a reference (`src/worker.rs`, `crates/macros/state.rs`). The racing edits are run by `edits_follow_the_model` in `tests/conformance.rs`.

**What it does not cover.**

- The mpi backend. `miri.sh` builds with `--features seq` only.
- `math::`. It has no unsafe code. The script's comment says Miri perturbs std's `exp` and `ln` by a few ulp, and these tests compare against those values; I did not re-check that claim.
- Three tests ignored under Miri. `sched::tests::contested_shares_follow_weights`: 50 000 starts to resolve a share to 0.001, hours under Miri. `backends::seq::tests_leader_stuck::wait_with_a_worker_that_waits_for_a_write_nothing_makes_panics`: the run is dropped with a worker stuck, so its thread outlives the test, and Miri refuses a main thread that ends first. `shares_follow_weights` in `tests/conformance.rs`: a statistic over 200 ms of steps.
- Smaller loops under `cfg!(miri)`, so fewer interleavings are explored: `src/sched.rs` (seeds 2000 to 12; rounds 1000 to 60; weight-0 starts 1000 to 60), `src/backends/seq/tests_steps_stuck.rs` (rounds 200 to 4), `tests/parallel.rs` (cases 1000 to 20), `tests/alloc.rs` (items 1000 to 100; repeats 100 to 3; ticks 1000 to 20), `tests/conformance.rs` (seeds 300 to 2; a control's steps 5 + up to 40 to 5 + up to 10; spin 200 000 to 200; disjoint steps 50 to 5). `src/backends/seq/tests.rs` raises the 10 s hang guard to 600 s; it is not a loop.
- Wall-clock tests. The script's header says a wait for a deadline is virtual time. I did not check Miri's clock model. Any test that measures elapsed time measures Miri under it, so its numbers mean nothing there.
- Kani harnesses for `sched.rs` (may_start, choose) and the segment allocator are not ported to this tree
  (branch `wm-proofs`, `proofs/kani/`).
