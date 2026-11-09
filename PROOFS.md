# Proofs

Each clause of the backend contract is checked by one or more of the methods below. A method checks a model
of the backend, or some of its runs, never the backend whole. Each entry names the method and the
point where it stops. A clause that nothing checks says so.

## Methods

**Model.** `sched.rs` is the scheduler as pure Rust: the premises of Start, the effects of End,
holds, edits and stops. Its unit tests check the rules on small cases, and a property test checks
that `choose` is the best-ranked step that `may_start` admits.

**Replay.** `sched::check` replays a backend's trace against the model and names the first event the
model forbids. With `exact`, it also requires the model's choice at each start. `tests/conformance.rs`
runs random workloads, edits included, and replays every trace. A replay shows that one run was
allowed. It cannot show that every run is.

**Outcome check.** `Run::__join_checked` compares each worker's result with the model's final state.
A `Deadlock` must be a state where no step can start, even after every deadline.

**Witness.** In debug builds, `Core` records the fields that running steps actually hold, apart from
any scheduler, and panics on a conflict.

**Programs.** `check/programs/*.rs` print their observations, and `check/programs/run.sh` compares each backend's
output with `check/programs/*.expected`.

**Model checking.** `check/proofs/mpi` holds TLA+ models of the MPI backend's protocols, checked by TLC
with `check/proofs/mpi/run.sh`. A model is checked, not the code. The model follows the code by reading.

**Measurement.** A statistical clause is measured: a run, a command, a machine and a tolerance.

**Driver.** `crates/driver/` checks user code at compile time (`crates/driver/README.md`). It trusts trame's own
code.

**Mutation.** A check that claims a clause must fail on a backend that breaks it. A mutant is that
backend.

## Clauses

**R1 Safety.** Replay, on the scheduling events. Programs, on fixed programs. No check covers every
label of T.

**R2 Progress.** No check proves it. Programs terminate with their expected output. The TLA+ models
check liveness of the MPI wake-up protocol and of a lane between hosts.

**L1 Refusal.** Programs `refuse` and `refuse_dup`, and the seq launch tests. On MPI, every refusal
is decided by a collective reduction (`src/backends/mpi/open.rs`).

**L2 Agreement.** `agree` in `src/backends/mpi/open.rs`. No test gives two processes different plans.

**L3 Building.** By construction: `launch::build_all` on seq, `imp.rs` on MPI.
`tests/no_alloc.rs` checks that a launch is refused when the builders cannot reach the domain.

**S1 Exclusion.** Model, replay, witness. A mutant that edits while a step runs is killed by the
model's assertion in `Sched::edited`.

**S2 Start.** Replay, exact on seq, safety only elsewhere.

**S3 Wake.** Replay, and the outcome check: a run that ends in `Deadlock` must be one where nothing
could start. On MPI, the TLA+ model `Epoch` checks that no wake-up is lost with one to three sleepers
per word. A mutant that forgets deadlines on seq is killed by the outcome check.

**S4 Edit.** Model; replay of runs with racing edits (`edits_follow_the_model`), in which every step
asserts the pool it reads is whole. The MPI edit handshake has no model.

**S5 Stop.** Model.

**S6 Priority.** Model: a writer of weight 1 against two overlapping readers of weight 0.1 starts.
Replay, since S6 is part of `may_start`.

**S7 Shares.** Model (`contested_shares_follow_weights`); measurement (`shares_follow_weights`,
`examples/weights_time.rs`). No backend states its tolerance.

**S8 Failure.** Model; programs `err` and `err_done`.

**S9 Trace.** Replay checks the traces it is given. Nothing checks that a trace is complete.

**T1 Order, T2 Capacity, T3 Delivery.** Programs; on MPI, `src/backends/mpi/tests` (FIFO with
verification, overwrite, fairness, large routes, node splits).

**P1 Writer.** By type: a step takes the pool as `&P`, and `#[trame::step]` refuses `&mut`.

**P2 Edit.** As S4; program `pool_edit`.

**M1 Placement.** On seq the domain is the heap. On MPI, by construction (`segment.rs`).
No check walks a built state to find a byte outside its domain.

**M2 Meaning, M3 Mutation.** By type: `#[trame::state]` requires `Plain` of each field and
`State::Pool`. The rejection suite (`check/rejections/cases/plain/`) refuses each excluded kind, nested
under user types too, and accepts plain nesting. The check is on types: a `usize` holding an
address is plain, and an `unsafe impl Plain` is not checked.

**M4 Allocation.** `tests/alloc.rs` counts zero allocations over 999 rounds of a step and over every
call of `parallel!`. The driver refuses allocation in items, not in steps.

**M5 Visibility.** As S4 within a process.

**M6 Bound.** On MPI by construction: `segment::alloc` answers null past the segment's end, and
`Alloc` writes through no null (`domain.rs`). No test fills a domain.

**M7 Lifetime.** By construction. Threads: every agent holds its worker's slot, which holds the
state. Seq: the workers run only inside the leader's calls on the run. MPI: `leader.rs` `Leader` holds
the plan's references until `join`, and leaks them when the run is dropped without it. Replay:
`fanin_fifo` drops its handles after `set`; a leader that freed the blocks then would make its
workers read the segment's free list as their state, and the run would hang. No test drops a run
without its join on MPI.

**F1 Identity.** By construction: `hosts.rs` numbers hosts by their lowest world rank. Program
`peers` on two hosts.

**F3 Port agreement.** Program `refuse_ports` on two hosts: a capacity that differs refuses both,
naming the route (`cross.rs` `connect`). No test declares a route on one host only, which waits.

**F4 Tag.** By construction (`io.rs` `tag`): a hash of the type's path and size. Programs `peers`
and `xhost_ring` on two hosts compile each type once, so they do not test two compilations.

**F5 Routes between hosts.** Program `xhost_ring` on two hosts: 2000 items per worker over lossless
lanes of capacity 4, each receiver asserting order and once-only delivery, and lossy ticks asserted
to increase. TLA+ model `Lane` (`check/proofs/mpi/Lane.tla`): order, room by credits, every frame finds
its posted receive, every frame taken and credited. Lossy lanes and the end have no model.

**F6 Peer channel.** Program `peers` on two hosts.

**E1 Abort.** Programs `abort`, `abort_join`, `leader_panics`.

**E2 Join.** Programs; the outcome check. The detector of stuck runs across hosts
(`src/backends/mpi/detect.rs`) is modelled in `check/proofs/mpi/Detect.tla`: `MCDetect` checks that a Stuck
verdict is given only when every worker is quiet or done and nothing is in flight. `MCDetectOnce`,
the mutant that skips the comparison with the previous round, must violate it, and does. Programs
`xhost_deadlock` and `leader_stuck` run it on two hosts. The model has one worker per host, H = 2, three
rounds and no timeouts.

## Conformance

A finite trace can show that R1 fails, never that R2 holds. Replay therefore checks safety on the
runs it sees, and progress rests on the programs ending and on the TLA+ liveness properties.

## Latency

T leaves latency to the environment, so no proof says anything about it. The MPI backend has a
recorder, `src/backends/mpi/tests/latency.rs`, which writes round trips between workers in the nixsci
record format and computes no verdict. No latency claim is checked: the tree holds no nixsci
experiment that locks those records by content hash. A claim would have this form: the fraction of
round trips whose half exceeds the fastest by more than 100 µs is below 0.1%, at confidence 0.99.

## Unsafe code

Each `unsafe` block in trame states in a `SAFETY` comment why it is sound, and names the clause
it relies on. No tool checks trame's own `unsafe` code: the driver trusts it.

## Assumptions

**Shared memory across processes.** The MPI backend's processes share memory through mappings of
one object. The Rust memory model is defined within one process. The backend assumes that atomics
in a shared mapping order memory across processes as they do across threads, which holds on the
hardware it runs on and is not a guarantee of the language.

**Foreign calls.** MPI and the Linux futex and shared-memory calls behave as documented.

**Hardware.** Every agent that is not blocked keeps running: the hypothesis of R2.
