# Proofs

Each clause of `CONTRACT.md` is checked by one or more of the methods below. A method checks a model
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

**Programs.** `programs/*.rs` print their observations, and `programs/run.sh` compares each backend's
output with `programs/*.expected`.

**Model checking.** `proofs/mpi` holds TLA+ models of the MPI backend's protocols, checked by TLC
with `proofs/mpi/run.sh`. A model is checked, not the code. The model follows the code by reading.

**Measurement.** A statistical clause is measured: a run, a command, a machine and a tolerance.

**Driver.** `driver/` checks user code at compile time (`driver/README.md`). It trusts trame's own
code.

**Mutation.** A check that claims a clause must fail on a backend that breaks it. A mutant is that
backend.

## Clauses

**R1 Safety.** Replay, on the scheduling events. Programs, on fixed programs. No check covers every
label of T.

**R2 Progress.** No check proves it. Programs terminate with their expected output. The TLA+ models
check liveness of the MPI wake-up, credit and abort protocols.

**L1 Refusal.** Programs `refuse` and `refuse_dup`, and the seq launch tests. On MPI, every refusal
is decided by a collective reduction (`backends/mpi/open.rs`).

**L2 Agreement.** `agree` in `backends/mpi/open.rs`. No test gives two processes different plans.

**L3 Building.** By construction: `launch::build_all` on seq and threads, `imp.rs` on MPI.
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

**T1 Order, T2 Capacity, T3 Delivery.** Programs; on MPI, `backends/mpi/tests` (FIFO with
verification, overwrite, fairness, large routes, node splits) and the TLA+ model `Credit`.

**P1 Writer.** By type: a step takes the pool as `&P`, and `#[trame::step]` refuses `&mut`.

**P2 Edit.** As S4; program `pool_edit`.

**M1 Placement.** On seq and threads the domain is the heap. On MPI, by construction (`segment.rs`).
No check walks a built state to find a byte outside its domain.

**M2 Meaning, M3 Mutation.** Not checked.

**M4 Allocation.** `tests/alloc.rs` counts zero allocations over 999 rounds of a step and over every
call of `parallel!`. The driver refuses allocation in items, not in steps.

**M5 Visibility.** As S4 within a process.

**M6 Bound.** Not checked.

**E1 Abort.** Programs `abort`, `abort_join`, `leader_panics`; TLA+ model `Abort`.

**E2 Join.** Programs; the outcome check.

## Unsafe code

Each `unsafe` block in trame states in a `SAFETY` comment why it is sound, and names the clause
it relies on. No tool checks trame's own `unsafe` code: the driver trusts it.

## Assumptions

**Shared memory across processes.** The MPI backend's processes share memory through mappings of
one object. The Rust memory model is defined within one process. The backend assumes that atomics
in a shared mapping order memory across processes as they do across threads, which holds on the
hardware it runs on and is not a guarantee of the language.

**Foreign calls.** MPI and the Linux futex and shared-memory calls behave as documented.

**Hardware.** Every agent that is not blocked keeps running (`spec.md`, Requirement (Progress)).
