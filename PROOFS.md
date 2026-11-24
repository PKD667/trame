# Proofs

Each clause of the backend contract is checked by one or more of the methods below. A method checks a model
of the backend, or some of its runs, never the backend whole. Each entry names the method and the
point where it stops. A clause that nothing checks says so.

Where a check belongs to one backend, this file says so and names the place in `src/backends/README.md`
(section Checks by backend) that lists it. The per-backend inventory of every clause — what checks it, where,
and what it does not — is the guarantees map (`src/backends/README.md`, generated from
`src/backends/<backend>/guarantees.tsv` by `check/guarantees.py`, checked by the verdict `guarantees`).

## Methods

**Model.** `sched.rs` is the scheduler as pure Rust: the premises of Start, the effects of End,
holds, edits and stops. Its unit tests check the rules on small cases, and a property test checks
that `choose` is the best-ranked step that `may_start` admits.

**Replay.** `sched::check` replays a backend's trace against the model and names the first event the
model forbids. With `exact`, it also requires the model's choice at each start. `tests/conformance.rs`
runs random workloads, edits included, and replays every trace. A replay shows that one run was
allowed. It cannot show that every run is.

**Outcome check.** `Run::__join_checked` compares each worker's result with the model's final state.
A `Deadlock` must be a state where no step can start.

**Witness.** In debug builds, `Core` records the fields that running steps actually hold, apart from
any scheduler, and panics on a conflict.

**Programs.** `check/programs/*.rs` print their observations, and `check/programs/run.sh` compares each backend's
output with `check/programs/*.expected`.

**Model checking.** The TLA+ models of a backend's protocols live in `src/backends/<backend>/proofs/` and run with
TLC, through the run script of that directory (the verdict `protocol-<backend>`). A model is checked, not the code.
The model follows the code by reading.

**Measurement.** A statistical clause is measured: a run, a command, a machine and a tolerance.

**Driver.** `crates/driver/` checks user code at compile time (`crates/driver/README.md`). It trusts trame's own
code.

**Mutation.** A check that claims a clause must fail on a backend that breaks it. A mutant is that
backend.

## Clauses

**R1 Safety.** Replay, on the scheduling events. Programs, on fixed programs. No check covers every
label of T.

**R2 Progress.** No check proves it. Programs terminate with their expected output. Some backends have
TLA+ models of their wake-up protocol and of their lanes between hosts (`src/backends/README.md`).

**L1 Refusal.** Programs `refuse` and `refuse_dup`, and each backend's launch tests. On a backend whose
host is several processes, every refusal is decided on all of them together (`src/backends/README.md`).

**L2 Agreement.** Each backend has an agreement check for its plans (`src/backends/README.md`). No test
gives two processes different plans.

**L3 Building.** By construction in each backend's builder path (`src/backends/README.md`).
`tests/no_alloc.rs` checks that a launch is refused when the builders cannot reach the domain.

**S1 Exclusion.** Model, replay, witness. A mutant that edits while a step runs is killed by the
model's assertion in `Sched::edited`.

**S2 Start.** Replay, exact on a backend whose choice is `Sched::choose`, and safety only elsewhere.

**S3 Wake.** Replay, and the outcome check: a run that ends in `Deadlock` must be one where nothing
could start. A backend with a multi-process wake-up protocol has a TLA+ model of it
(`src/backends/README.md`).

**S4 Edit.** Model; replay of runs with racing edits (`edits_follow_the_model`), in which every step
asserts the pool it reads is whole. A backend whose edits cross processes has its own handshake, which the
model `MCEdit` checks at small bounds (`src/backends/README.md`); its larger configurations did not finish.

**S5 Stop.** Model.

**S6 Priority.** Model: a writer of weight 1 against two overlapping readers of weight 0.1 starts.
Replay, since S6 is part of `may_start`.

**S7 Shares.** Model (`contested_shares_follow_weights`); measurement (`shares_follow_weights`,
`examples/weights_time.rs`). No backend states its tolerance.

**S8 Failure.** Model; programs `err` and `err_done`.

**S9 Trace.** Replay checks the traces it is given. Nothing checks that a trace is complete.

**T1 Order, T2 Capacity, T3 Delivery.** Programs, and the transport checks of each backend that has
its own (`src/backends/README.md`).

**P1 Writer.** By type: a step takes the pool as `&P`, and `#[trame::step]` refuses `&mut`.

**P2 Edit.** As S4; program `pool_edit`.

**M1 Placement.** Every backend places a state in its domain by construction, and the placement of each is in
`src/backends/README.md`. No check walks a built state to find a byte outside its domain.

**M2 Meaning, M3 Mutation.** By type: `#[trame::state]` requires `Plain` of each field and
`State::Pool`. The rejection suite (`check/rejections/cases/plain/`) refuses each excluded kind, nested
under user types too, and accepts plain nesting. The check is on types: a `usize` holding an
address is plain, and an `unsafe impl Plain` is not checked.

**M4 Allocation.** `tests/alloc.rs` counts zero allocations over 999 rounds of a step and over every
call of `parallel!`. The driver refuses allocation in items and in steps. An item refuses every call into
`alloc`. A step refuses every such call except one that its list names by the alloc type's own key
(`Vec::len`, `Vec::index_mut`), so a step may rewrite the buffers its state holds but not grow, make or free
one. `check/rejections/cases/allocation/` holds the pairs: `vec_push_step.rs` is refused,
`vec_index_assign_step_ok.rs` is accepted. The rule is in `crates/driver/src/check/mod.rs` (`judge`,
`allocates`). A raw allocation inside trame's own code is not a call into `alloc`, and it is not seen
(`crates/driver/README.md`, line 68).

**M5 Visibility.** As S4 within a process.

**M6 Bound.** Each backend's allocator answers null past its domain's end, by construction
(`src/backends/README.md`). `Alloc` writes through no null (`domain.rs`). No test fills a domain.

**M7 Lifetime.** By construction. Every agent holds its worker's slot, which holds the
state. A backend whose workers run inside the leader's calls, and one whose processes outlive a run,
keep the states live until the join (`src/backends/README.md`). Replay: `fanin_fifo` drops its handles
after `set`; a leader that freed the blocks then would make its workers read the domain's free list as
their state, and the run would hang. No test drops a run without its join on a multi-process backend.

**M8 Share.** Programs `share` (the number a builder reads, the same in a step) and `share_refused` (a builder
past its share refuses the launch), and the unit test of the rule in `src/system.rs`. The reading of the host's
memory is the backend's own, by construction (`src/backends/README.md`); nothing checks the number against the
machine's real memory, and the guarantee is for builders only, not for growth in edits.

**F1 Identity.** By construction in each backend's host numbering (`src/backends/README.md`). Program
`peers` on two hosts.

**F3 Port agreement.** Program `refuse_ports` on two hosts: a capacity that differs refuses both,
naming the route. Program `route_one_side` on two hosts: a route that only host 0 declares is refused on
host 0, with a message that names the route, after host 0 waits until host 1 ends
(`route_one_side.h2.expected`). Host 1 returns. On one host the route is not declared, and the worker runs
(`route_one_side.expected`).

**F4 Tag.** By construction: a hash of the type's path and size (`src/backends/README.md`). Programs `peers`
and `xhost_ring` on two hosts compile each type once, so they do not test two compilations.

**F5 Routes between hosts.** Program `xhost_ring` on two hosts: 2000 items per worker over lossless
lanes of capacity 4, each receiver asserting order and once-only delivery, and lossy ticks asserted
to increase. A TLA+ model of a lane covers order, room by credits, every frame finding its posted
receive, and every frame taken and credited (`src/backends/README.md`). Lossy lanes and the end have
no model.

**F6 Peer channel.** Program `peers` on two hosts.

**E1 Abort.** Programs `abort`, `abort_join`, `leader_panics`.

**E2 Join.** Programs; the outcome check. Where a backend detects stuck runs across hosts, the detector has
the model `Detect`, at its bounds (where it lives is in `src/backends/README.md`, Checks by backend). Programs `xhost_deadlock` and
`leader_stuck` run the detector on two hosts. They test its verdicts; they do not prove them.

## Conformance

A finite trace can show that R1 fails, never that R2 holds. Replay therefore checks safety on the
runs it sees, and progress rests on the programs ending and on the TLA+ liveness properties.

## Latency

T leaves latency to the environment, so no proof of T says anything about it. Latency is checked statistically,
by the lab's latency experiment: round trips with random gaps between them, between workers on one host and
across two, at several message sizes. Round trips in one run are autocorrelated, so each run is one sample, and
the test counts runs, not round trips. A claim is a proportion: the fraction of round trips over the p99 bound
is below 0.01, judged with `nixsci.stats` over the runs.

The bounds are the Q clauses of the backend contract, set per placement and size in `lab/latency/q.nix` for a named
reference deployment. A bound that is still null is stated as absent: the experiment measures that placement and
size, and no claim is judged on it.

## Unsafe code

Each `unsafe` block in trame states in a `SAFETY` comment why it is sound, and names the clause
it relies on. Miri runs a backend's tests under Tree Borrows (`check/proofs/miri.sh BACKEND`), for a backend whose tests cargo runs. It found a data race
in the pool edit, fixed in commit f7fc1a5. It does not run a backend that needs a launcher. The boundaries and the how-to are in
`check/proofs/README.md`, section Unsafe code and Miri.

## Assumptions

**Shared memory across processes.** A backend whose processes share memory does so through mappings of
one object. The Rust memory model is defined within one process. Such a backend assumes that atomics
in a shared mapping order memory across processes as they do across threads, which holds on the
hardware it runs on and is not a guarantee of the language.

**Foreign calls.** The system calls that a backend uses behave as documented.

**Hardware.** Every agent that is not blocked keeps running: the hypothesis of R2.
