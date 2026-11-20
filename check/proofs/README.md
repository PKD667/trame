# Proofs

What is checked by a model, and how unsafe code is held to account. The model checks of a backend's protocols are in that
backend's directory (`src/backends/<b>/proofs/`, with a README of their own); a backend that has none has none. A model is an
abstraction: a passing check says the abstraction has the property at the bounds shown, not that the code has it.

## Unsafe code and Miri

- `unsafe-registry.sh ROOT` lists every `unsafe` site in the Rust sources and fails on a site with no `SAFETY:` comment
  citing a clause of `CONTRACT.md`. It reads source only. It is a measure, not yet a gate: no check runs it, and many
  sites still fail it. `sh check/proofs/unsafe-registry.sh .` prints them.
**What runs.** `miri.sh BACKEND` runs the tests of the backend named (one whose descriptor has `cargoTests = true`) under Miri with Tree Borrows and isolation on: the library's unit tests except `math::`, `tests/conformance.rs`, `tests/parallel.rs` and `tests/alloc.rs`. The four suites run as separate jobs, each with its own target directory. The script exits non-zero if any job fails, and prints the names of the failed jobs.

**How to run.** From the repository root, `nix develop .#miri --command sh check/proofs/miri.sh BACKEND`. Read the exit status directly: a pipe (`| tail`) reports the status of its last command, not of the script. Expected counts: lib 52 passed and 2 ignored, conformance 5 passed and 1 ignored, parallel 4, alloc 2. It exited 0 on three runs; no failing run was produced, so the non-zero path is untested.

**Cost.** 82 s and 87 s with the target directories warm, and 96 s on the first run, which builds the dependencies. Machine: 12 threads (`nproc`), AMD Ryzen 5 PRO 4650U; `cargo miri --version` gives `miri 0.1.0 (66da6cae1a 2026-04-20)`, rustc `1.97.0-nightly` (66da6cae1, 2026-04-20). Another job shared the machine, so the times are noisy by several seconds. The serial run before the split took 219 s on the same tree.

**What it found.** Commit f7fc1a5 fixes a data race that Miri reported (the commit message says so; which test reported it first is not recorded). Before the fix, `State::run` took the pool as `&P`, so a step that did not take the pool still held a `&P` while the leader's edit wrote the pool through `&mut P`. CONTRACT.md P2 forbids that. Now `State::run` takes `*const P`, and only a step with the POOL bit makes a reference (`src/worker.rs`, `crates/macros/state.rs`). The racing edits are run by `edits_follow_the_model` in `tests/conformance.rs`.

**What it does not cover.**

- Backends that need a launcher: Miri runs only the tests cargo itself can run (`cargoTests` in the descriptor).
- `math::`. It has no unsafe code. The script's comment says Miri perturbs std's `exp` and `ln` by a few ulp, and these tests compare against those values; not re-checked.
- Three tests ignored under Miri. `sched::tests::contested_shares_follow_weights`: 50 000 starts to resolve a share to 0.001, hours under Miri. `tests_leader_stuck::wait_with_a_worker_that_waits_for_a_write_nothing_makes_panics`: the run is dropped with a worker stuck, so its thread outlives the test, and Miri refuses a main thread that ends first. `shares_follow_weights` in `tests/conformance.rs`: a statistic over 200 ms of steps.
- Smaller loops under `cfg!(miri)`, so fewer interleavings are explored: `src/sched.rs` (seeds 2000 to 12; rounds 1000 to 60; weight-0 starts 1000 to 60), `tests_steps_stuck.rs` of the backend (rounds 200 to 4), `tests/parallel.rs` (cases 1000 to 20), `tests/alloc.rs` (items 1000 to 100; repeats 100 to 3; ticks 1000 to 20), `tests/conformance.rs` (seeds 300 to 2; a control's steps 5 + up to 40 to 5 + up to 10; spin 200 000 to 200; disjoint steps 50 to 5). `tests.rs` of the backend raises the 10 s hang guard to 600 s; it is not a loop.
- Wall-clock tests. The script's header says a wait for a deadline is virtual time. Miri's clock model was not checked. Any test that measures elapsed time measures Miri under it, so its numbers mean nothing there.
- Kani harnesses for `sched.rs` (may_start, choose) and the segment allocator are not ported to this tree
  (branch `wm-proofs`, `proofs/kani/`).
