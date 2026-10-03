# Execution

The [backend contract](backend.md) defines logical execution, communication and sharing.
The process interface and NERVE integration boundary below are targets. Recorded implementation
checks establish only the coverage they exercised.

## Process declarations

A process owns retained state and a bounded `step` method, normally taking `&mut self`.

```rust
#[trame::process]
struct Intake<'worker, 'graph> {
    worker: &'worker Bunch<'graph>,
    // Retained receive state and borrowed endpoints.
}

impl Intake<'_, '_> {
    fn step(&mut self) -> Result<trame::Step, WorkerFailure> {
        // The application's bounded intake step.
    }
}

let intake = Intake::new(/* ... */);
let delivery = Delivery::new(/* ... */);
let transport = Transport::new(/* ... */);

trame::concurrent!(transport, intake, delivery)?;
```

Construction order and arm order are explicit. NERVE retains transport, intake, delivery order
because arm order participates in failure selection. The caller needs no mutable bindings or
`move` closures.

### Attribute expansion

`#[trame::process]` preserves the struct and implements the shared marker `trame::Process`.
A struct attribute cannot inspect a separate inherent `impl`; Rust checks the `.step()` call and
infers its error type at invocation.

```rust
impl<'worker, 'graph> ::trame::Process for Intake<'worker, 'graph> {}

let mut state = ::trame::__process(intake); // Checks Process + Send; evaluates once.
let mut arm = ::trame::run::arm(move || state.step());
```

The marker has no associated `Error` or trait `step`. A compatible shared receiver is valid;
a missing or incompatible inherent method fails at invocation.

The attribute accepts named, tuple and unit structs. It retains fields, visibility, attributes,
lifetimes, type/const parameters, bounds and where-clauses. Generated impls strip generic defaults.
Enums, unions and functions are outside this declaration. Fields require no `Copy` bound.

### Binding and stepping

Each expression is evaluated once, left to right, before any arm begins. The macro owns each
value. A marker implementation for `&mut P` permits borrowing a declared process whose state
must remain available afterwards.

Binding requires `Process + Send`. `step(&mut self)` returns `Result<Step, E>`; all arms use one
compatible error type. The attribute adds no serialization or error-representation bound.

`Progress`, `Idle` and `Done` retain their meanings. No step follows `Done`. An idle process
permits other unfinished processes to advance. Sequential cooperative stepping is valid.
Boundedness is a caller obligation. The nv runner's warp-uniformity constraint requires an
implementation fix; it is outside the target application contract.

Objects remain alive until the arms join or finish. Owned host objects drop on the calling side
after the join, including when a panic is resumed. A device trap promises no Rust destructors.
Borrowed resources remain the caller's responsibility.

### Scoped transport

```rust
trame::concurrent!(cx;
    recv(TAG_SYS) => control,
    recv(..) => intake,
    delivery,
)?;
```

This form calls `step(&mut self, io: &mut trame::Io<'_>)` through the existing `arm_io` adapter.
The `Io` borrow lasts one step and cannot escape through safe code. Its endpoints are
`io.{send, lead, recv, flush}`. First-match tag ownership, FIFO and `NotReceiving` remain unchanged.
Worker `trame::{send, recv, flush}` and `leader::{send, recv}` remain separate endpoints.
NERVE retains its transport arm.

The process-facing interface accepts process objects throughout callers and verification
fixtures. Closures are private runner machinery. The public macro
has one dispatch grammar and does not infer whether an identifier names a closure or a struct.

## Layer boundary

Dependencies run NERVE → trame → selected backend. Trame modules may not name `Bunch`, `Spike`,
`SystemMsg`, `Work`, neurons or learning rules.

| NERVE retains | trame owns |
|---|---|
| Worker, graph, model clock and reservoir policy | Process binding, scheduling, joins and execution lowerings |
| Message encoding, revision validity and stop meaning | Opaque byte storage ownership and handoff |
| Synaptic windows and staging ages | Retained transport records and acceptance cursors |
| Learning, firing, wake invalidation and observation meaning | Routing, refusal, publication and collective mechanics |

Trame's storage boundary covers the ownership contract in `src/nerve/pool.rs` and generic
block/cursor parts of `bunch/transport.rs`. NERVE owns its three arms, stop-message decoding
and revision handling.

An owned block occupies exactly one location: spare storage, producer, queue or consumer.
A refused handoff returns the same block. A transport cursor advances only after acceptance.
Pool indices, partial-block offsets and return bookkeeping belong below neuronal code.
Missing capacity remains an explicit refusal across backend storage representations.

Portable device buffers require the same capacity and lifetime guarantees as CPU storage;
`Vec<u8>` alone supplies no device-buffer contract. `NoUninit` payloads do not make an arbitrary
Rust owner safe to duplicate across lanes. Storage and process declarations have separate
acceptance requirements.

## Integration functions

The target function operates on target-owned state:

```rust
#[trame::parallel]
#[trame::ordered(key = frame.target: Target)]
fn integrate(
    frame: Frame,
    neuron: &mut Slot,
    pass: &Integration,
) -> Result<(), WorkerFailure> {
    // Target-owned transition; no direct wire or shared-log effects.
}
```

The frame target indexes the item's only mutable neuron state. `Integration` contains shared
read-only inputs and result-publication primitives. Keys identify dependency domains; other
effects retain their own ordering requirements.

### Target ownership

Slots stored as `Vec<Exclusive<Slot>>` and shared with intake queries cannot be borrowed as an
exclusive `Keyed` slice through `&Bunch`. Target ownership must precede kernel invocation.
A shared borrow cannot manufacture `&mut Slot`. Queries must respect the publication cut and
cannot observe a partially published parallel transition.

Learning `pre`/`post`, sequence allocation and wake generations belong to each target's ordered
history. Worker-wide `Box<dyn Learning>` must be separated by target or remain outside the
parallel region while preserving that history. Sorting or dropping frames resolves neither
ownership dependency.

Results retain original item position and crossing order. Their owner publishes reservoir
updates, fanout, lane staging and logs under an explicit ordering rule. Invalid targets retain
NERVE's reported-drop policy; fatal `Keyed::OutOfRange` would change it. AlphaLif can produce
unbounded crossings per advance. Results must retain every crossing.

### Admission and backpressure

The scalar pass retains a bounded extracted list. `transport::drain` gates each frame before its
neuron changes; a blocked handoff stops subsequent integration. `invoke!` instead finishes every
item and offers no suspend-and-resume result.

Process binding preserves per-frame admission, failure behavior, reset cuts and incomplete-pass
accounting. Whole-pass `invoke!` requires a separate admission, result-storage and publication
contract. Hidden unbounded effect queues, implicit replay of mutated items and
integrate-everything-then-send transformations are forbidden.

Every admitted frame is accounted for exactly once. Same-target history is preserved; unaccepted
output is never reported accepted. Reset distinguishes published effects from discarded work.
Frame-versus-batch admission, cross-target ordering and state-query cuts are unresolved
requirements for a full NERVE GPU kernel.

### Numerical semantics

Frame deadlines and integration instants are `f64` model time. Resolved weights and membranes
are `f32` where the model defines them. LIF computes its exponential in `f64` before converting
decay to `f32`. Process binding preserves these boundaries, clock sampling, thresholds, reset
rules and arithmetic order.

SIMD and CUDA kernels require separate numerical evidence. Scheduling changes authorize no
replacement exponential, FMA contraction, precision change or altered threshold decision.
Recorded reproducible inputs must be compared through state, crossings, sequence numbers and
errors, beyond final classification. This design claims no speedup.

## Execution lowerings

### Host

`parallel` and `ordered` run inline on the caller's thread in list order. This preserves per-key
order and selects the first list-position error without grouping. A threaded lowering requires
a measured crossover on the real ordered body. Stable key grouping belongs with its SIMD
consumer. No real-body speedup is established.

### NV host model

The host-model port uses an inline list-order loop for both item declarations and a round-robin
bounded-step process scheduler. Shared context is `&C: Sync`; items, slots and errors retain their
public `Send` bounds. Every item runs once, including items after failure. The caller retains the
first list-position error and drops later errors. Repeated keys borrow their one slot in list
order. Infallible bodies use this loop with `Infallible`.

`sync::with` returns the callback's owned result. Handoff moves values into initialized ready/spare
slots, returns unaccepted values on refusal and drops retained values once. Partial construction
records initialized spares for host unwind. Neither primitive requires `NoUninit` or cross-lane
word copying. Atomics use nv's own implementation.

Host-model coverage establishes neither CUDA legality nor resident launch admission, production
W1 receive lowering or W5 device ownership. The warp-entry port assigns warp identity and enters
a barrier before owner selection; its peer and leader transport require full-warp convergence.
That mapping needs W2 ownership established before entry, with matching transport, barrier and
atomic address domains. Inline runners and owned primitives run once per logical owner.
Scalar execution is the primary lowering and carries no acceleration claim.

### NV device owner

The lane-zero lowering assigns ownership before worker entry. `init` refuses other lanes before
touching a barrier. One block contains the bounded cohort, with one physical warp slot per owner.
Rings and generation barriers use scalar acquire/release operations on launch-owned global memory.
Caller-local buffers and invocation outcomes remain with their owner. Cross-owner sync backing
and CPU-mapped leader admission require separate device evidence.

Plain receive uses `Owner::All`; device constants cannot hold borrowed receive-setting slices.
Scoped receive borrows the ordered arms and retains first-match ownership.

## Implementation requirements

Each implementation must satisfy these ownership requirements. The contract is defined
independently of implementation verification.

| Work | Acceptance requirement |
|---|---|
| Contract definition | Logical semantics remain independent of hardware mapping or broadcast representation. |
| Process declaration and `concurrent!` callers | Preserve runner, arm order, outcomes, panic/join behavior and routes. |
| NERVE Intake, Delivery and Transport objects | Preserve frame order, admission, clock sampling, reset accounting, logs and wire bytes. |
| Generic block storage and acceptance cursors | Keep NERVE names above trame; CPU and device storage obey one contract. |
| Target-owned transitions and publication | Decide admission/query cuts; forbid keyed aliasing, stale-wake resurrection, changed learning history and clipped crossings. |
| Parallel kernel invocation | Establish hardware error/order semantics and assess numerical differences. |

Declaration coverage includes generic and borrowed processes, missing or incompatible `step`,
non-Send state, error-type disagreement and short-lived `Io` borrows. Runtime coverage includes
constructor-once behavior, persistent state, no step after `Done`, bounded refusal without replay
and failure/panic rules. Falsifiers retain their strength. NERVE process objects require equal
scalar event traces for fixed inputs; parallel kernels require separate evidence.

The delivery and integration documents describe absent machinery, including an invoked integration
pass and slot storage that differs from the implementation. That disagreement is unresolved.
The process interface requires no new async runtime, backend-specific NERVE module tree or
performance-tuned constants. Exploratory NV checks use isolated source snapshots.
Certification requires the contract; scalar execution is independently valid.

## Verification

Certification applies to a recorded source, build and execution environment.

| Boundary | Invariant |
|---|---|
| Deployment | Worker rank and launch identity remain distinct; leaders are not workers. |
| Routes | One attempt; refusal accepts nothing; short receive consumes nothing; accepted bytes no longer borrow the caller. |
| Ordering | FIFO within channel and arm ownership rules; no invented Message/Lane ordering. |
| Geometry | Declared pairs, frame size, sharing cohorts and collective membership are checked. |
| Lifetime | Buffer ownership is unique; shutdown has no unstated delivery requirement. |
| Execution | Every item runs despite item errors; keyed order and first-error selection retain their scope. |
| Processes | Steps are bounded; cancellation, joins and failures follow the arm contract. |
| Portability | Public signatures, bounds and documented capabilities agree. |

Records identify specification version, source manifest, compiler/dependency pins, features,
launch geometry, device identity and raw verdicts. Source changes invalidate evidence for the
changed implementation. Documentation changes cannot alter a failed verdict.

### Coverage obligations

- Host comparisons use one recorded compiler; the locked Nix shell is the proposed common
  environment. CUDA records its separate compiler.
- Missing participants, absent verdicts, nonzero exits and timed-out launches fail conformance,
  including timeouts after the last printed claim. Enlarging spin bounds cannot establish a pass.
- Public-surface checks distinguish explicit and automatic trait implementations. Compile witnesses
  cover signatures, bounds and capabilities across all five selections. HTML names alone are
  insufficient. Incidental compiler traits remain outside documented guarantees.
- Invocation checks cover failures at items 1 and 32, an earlier out-of-range key, repeated keys
  and work after errors. GPU results reach the logical caller; errors need no cross-lane replicas.
- Process checks cover exclusive access, refused handoff ownership, abandoned state, CPU panic joins
  and agreement at warp-wide control boundaries. Collective calls remain outside steps.
- CUDA hardware checks exercise device-compatible public contracts. Unsupported and absent checks
  remain unverified. This document authorizes no remote allocation.

`K1` does not exercise `ClockMismatch`; X2 covers only the default selection. The main claim body
omits some geometries, failure injections and public signatures. Host-model passes cannot certify
warp votes, cross-lane errors, device visibility or representation acceptance by the CUDA compiler.
RTX 2080 Ti transport, discovery and execution demonstrations do not certify the complete contract.

## Recorded evidence

### Conformance suite

`bash trame/scripts/conform.sh local` ran once on MIST, without source edits or retries, and
exited **1**. The dirty source snapshot had workspace HEAD
`7ae219e828b3f2a2fb98853e40cf12b43b58b205` and mpi-rma HEAD
`497d66e635fdafb11e8f9ecf28dd5c0b82fadb93`. Before/after manifests matched SHA-256
`ff2adbff4166f8fb7564d447269f71d61c35da92578f08cf0137b9df11169af7`, covering tracked and
untracked trame/mpi-rma sources and root build pins.

Artifacts: `/tmp/trame-freeze-20260925T200027/` contains `env.before`, `fp.before`, `fp.after`,
`conform.stdout`, `conform.exit` and `suite/`. Original output: `/tmp/tmp.ewOw01wGB2/`.
This is correctness evidence only.

The recorded matrix names `none`, `nv host model`, `mpi` and `lossy` but contains five results
per row. Backend alignment is unresolved; the sequences below retain the raw result order.

| Check | Recorded results |
|---|---|
| Main-body claims D1 through C3 | not applicable; pass; pass; pass; pass |
| M5: full route, then shutdown without delivery | not applicable; pass; fail; fail; fail |
| X1: unit tests | pass; pass; pass; pass; pass |
| X2: declaration fixtures | pass; not run; not run; not run; not run |
| P1: public surface | fail in comparison; fail in comparison; fail in comparison; not compared; not compared |

MPI-family launches used four workers and two leaders; the nv host model used four workers and
one leader. No CUDA run was made. Host-model rustc was 1.96.0-nightly (`55e86c996`);
`suite/nix.log` records rustc 1.96.0 (`ac68faa20`) and Open MPI 5.0.10 for MPI builds.

**M5: shutdown under pressure failed.** MPI-family logs report
`Ok(2047) after 2047 accepted sends to 1; entering done`; `Ok(2047)` means the next send was
`Full`. All four workers lacked M5 verdicts at the 150-second launch timeout.
`backends/mpi/context.rs::done` starts with buffer detachment, whose comment says it waits for
buffered sends. This is a candidate cause, not a stack-trace finding. A longer timeout, receiver
draining or removal of M5 would not verify shutdown without delivery.

**P1: public-surface comparison failed.** The none/nv diff lists `Io::UnwindSafe` versus
`!UnwindSafe` and an extra nv `Send`. None explicitly implements `Send`; the scraper reads only
synthetic impls and misses it. The none/mpi `Leader::Freeze` comparison also mixes compilers.
These observation faults require repair before attributing differences to backends. Public
capabilities must still agree.

**NV invocation: first-error selection failed.** The `nv/run.rs` implementation selects each lane's
own first error rather than the first list-position error. Its tests cover successful items only.
An index reduction can select a winner, but arbitrary `E` cannot be copied to every lane.
One logical result owner permits sequential execution without a device-transfer or padding-free
error bound. Accelerated lowerings must preserve that ownership and the public error bounds.

### Host scheduling probe

`lowering-design.md` records a persistent four-thread pool losing to inline execution at
64 to 4096 frames on MIST. This probe establishes no crossover on the real ordered body.

### Public-surface witnesses

`bash trame/scripts/surface.sh /home/pkd/code/agents/nerve-nv-20260929/target-W4/p1` used
rustc/rustdoc 1.96.0-nightly (`55e86c996`) and isolated targets. Each of five `all.html` inventories
contained 77 exports; nv, mpi and lossy had empty diffs against none. All 45 expected-refusal
fixtures passed with structured rustc codes and primary spans at the marked obligations.
The comprehensive positive fixture failed on all five selections. Exit status was 1:
five positive failures and 45 negative passes.

Failures include missing `io::*`, `leader::{send_to,recv_from,done}` and free
`sync::{with,handoff::*}`, plus `#[parallel]` refusing the documented shared `&C` context.
Command records, compiler identity, inventories, diagnostics and statuses are in that output
directory. Final log: `/home/pkd/code/agents/nerve-nv-20260929/W4-s12-final3.log`.

The Freeze-only display difference is also recorded in
`/home/pkd/code/agents/nerve-addr-20260928/A3-conform.log`. This verifier excludes synthetic-impl
lists because incidental compiler traits are outside the contract. That exclusion leaves the
positive-client failures unresolved. The failed P1 record remains evidence.

### Device receive witness

Production W1, `nv/measure/lowering.rs`, exercises public receive and `Io` paths. It passes on
an RTX 2080 Ti with cuda-oxide fork `nerve/enum-pointer-overlay` at `ff292052`.
The nv lead report contains the evidence. This witness does not establish full CUDA conformance.
