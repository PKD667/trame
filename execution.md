# Execution declarations and implementation verification

Objects own state, declarations name work, and trame chooses how that work executes. The target
[backend contract](backend.md) defines logical execution, communication and shared memory without
prescribing threads or GPU lanes. Its definition is separate from certification of an implementation.
This document records the proposed migration and the evidence against the preceding implementation;
unimplemented interfaces and failed checks are not presented as verified behavior.

## Baseline: 2026-09-25

`bash trame/scripts/conform.sh local` ran once on MIST, without source edits or retries, and
exited **1**. The tree was dirty: workspace HEAD `7ae219e828b3f2a2fb98853e40cf12b43b58b205`,
mpi-rma HEAD `497d66e635fdafb11e8f9ecf28dd5c0b82fadb93`. The before/after source manifests
matched, with SHA-256 `ff2adbff4166f8fb7564d447269f71d61c35da92578f08cf0137b9df11169af7`.
The manifests include tracked and untracked trame and mpi-rma sources and the root build pins.

Local evidence is in `/tmp/trame-freeze-20260925T200027/`: `env.before`, `fp.before`, `fp.after`,
`conform.stdout`, `conform.exit`, and `suite/`. The script's original output is
`/tmp/tmp.ewOw01wGB2/`. This is correctness evidence, not a performance measurement.

| Check | none | nv host model | mpi | rma | rma-lossy |
|---|---|---|---|---|---|
| Main-body claims D1 through C3 | not applicable | pass | pass | pass | pass |
| M5: full route, then shutdown without delivery | not applicable | pass | fail | fail | fail |
| X1: unit tests | pass | pass | pass | pass | pass |
| X2: declaration fixtures | pass | not run | not run | not run | not run |
| P1: public surface | fail in comparison | fail in comparison | fail in comparison | not compared | not compared |

The main launch used four workers and two leaders for the MPI family; nv's host model used four
workers and one leader. These are not identical deployment geometries. No CUDA run was made.
The recorded host-model compiler was rustc 1.96.0-nightly, commit `55e86c996`; `suite/nix.log`
records rustc 1.96.0, commit `ac68faa20`, and Open MPI 5.0.10 for the MPI builds.

### Implementation failures

**M5 refutes shutdown under pressure.** Each MPI-family pressure log says
`Ok(2047) after 2047 accepted sends to 1; entering done`. In the claim's code, `Ok(2047)` means
the next send answered `Full`. All four workers then failed to report an M5 verdict before the
150-second launch timeout. `shared/context.rs::done` begins with buffer detachment, which its
own comment says waits for buffered sends; this is the first implementation point to investigate.
The logs do not provide a stack trace. Increasing the timeout, draining on the silent receiver,
or dropping M5 would not verify the stated contract.

**P1 is red, but not every displayed difference is a type difference.** Its none/nv diff lists
`Io::UnwindSafe` versus `!UnwindSafe`, and includes an extra `Send` entry for nv. The none
implementation explicitly implements `Send`; the scraper reads only rustdoc's synthetic-impl
section and misses that implementation. The none/mpi `Leader::Freeze` difference also compares
different compilers. Repair the observation before attributing all its output to a backend;
retain the requirement that public capabilities agree.

**The current nv invocation violates first-error selection.** `nv/run.rs` returns the first
failure of each lane's own items, not one first failure in list order. A reduction can choose a
winning item index, but copying an arbitrary `E` to every lane is not valid ownership. The current
nv invocation tests exercise successful items, not this distinction.

This does not require a device-transfer bound in the public interface. A sequential invocation
with one logical result owner is valid. An accelerated lowering must preserve that ownership and
result, rather than force application errors into a padding-free representation. The earlier
recommendation to require a transferable error is withdrawn: it mistook one physical mapping for
the contract. Keep the first-error rule and reject an incorrect lowering, not a valid program.

### Host items run inline

The host runs `parallel` and `ordered` items on the caller's thread, in list order. That order
keeps each key's calls in order and makes the first error met the first in list order, so the
scalar lowering needs no grouping. Threads were dropped because none paid in the probes
(`lowering-design.md`: a persistent four-thread pool lost to inline at 64 to 4096 frames on MIST);
a threaded lowering returns only with a measured crossover on the real ordered body. Stable key
grouping belongs with the SIMD wave lowering that consumes it. No real-body speedup is claimed.

**CUDA conformance remains unverified.** The README records device transport, discovery and
execution demonstrations on an RTX 2080 Ti, not the complete public contract. A host-model pass
cannot establish actual warp votes, cross-lane error propagation, device memory visibility or
compiler acceptance of their representations.

## What verification certifies

Specify meanings and caller obligations independently of the experiments. Then verify each
implementation against that specification. Certification covers the following for each source,
build and execution environment whose verification is claimed:

| Boundary | Invariant |
|---|---|
| Deployment | Worker rank and launch identity remain distinct; leaders are not workers. |
| Routes | One attempt; refusal accepts nothing; short receive consumes nothing; accepted bytes no longer borrow the caller. |
| Ordering | FIFO within the contract's channel and arm ownership rules; no invented ordering between Message and Lane. |
| Geometry | Declared pairs, declared frame size, sharing cohorts and collective membership are checked. |
| Lifetime | Buffer ownership is unique; shutdown does not acquire an unstated delivery requirement. |
| Execution | Every invoked item runs despite item errors; keyed order and first-error selection have their stated scope. |
| Processes | Steps remain bounded; cancellation, joins and failures follow the existing arm contract. |
| Portability | Public signatures, bounds and documented capabilities agree, not just exported names. |

A verification record needs the specification version, source manifest, compiler and dependency
pins, feature sets, launch geometry, device identity where applicable, and raw verdicts. A source change invalidates the
record for the changed implementation. Documentation changes do not retroactively turn a red
run green. Do not commit unrelated work or call the present dirty tree a clean release.

### Verification gates

1. Run all host comparisons with one recorded compiler. Using the locked Nix shell for every
   host feature is the proposed choice; CUDA retains its separately recorded compiler.
2. Rerun the existing conformance claims after root-cause fixes. Missing participants, missing
   verdicts and every nonzero or timed-out launch must fail the gate, including a timeout after
   the last claim printed. Do not use a larger spin bound to obtain a pass.
3. Correct P1's treatment of explicit versus automatic trait implementations. Add compile
   witnesses for public signatures, generic bounds and capabilities across all five selections;
   HTML item names alone cannot establish them. Preserve the old P1 result and distinguish
   documented guarantees from incidental compiler traits when specifying its successor.
4. Verify invocation and step outcomes, including failures on items 1 and 32, an earlier
   out-of-range key, repeated keys, and observable work after an error. A GPU invocation must
   return the correct item error to its logical caller; replicating it across lanes is not required.
5. Verify process primitives: exclusive access, handoff ownership across refusals, abandoned
   state, CPU panic joining, and agreement at warp-wide control boundaries. Keep collective
   calls outside steps as the contract requires.
6. Run device-compatible public-contract checks on CUDA hardware. Report unsupported or absent
   checks as unverified, not as a host-model pass. No remote allocation is authorized by this
   design document itself.

`K1` currently does not exercise `ClockMismatch`, and X2 currently runs only the default
selection. The main claim body also does not cover every geometry, failure injection, or every
public method signature. These coverage limits remain visible even if its table becomes green.

## Struct-level process declaration

A process is retained state with a callable `step` method, normally taking `&mut self`. Declare
the struct, not a closure factory. The following is the proposed source interface; its application
bodies are omitted.

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

The construction order and arm order are explicit. The example retains NERVE's present arm order,
transport then intake then delivery, because that order participates in failure selection.
No mutable bindings or `move` closures are required at the application call site.

### How the decorator works

A struct attribute cannot inspect a separate inherent `impl`. It therefore does not infer an
associated error type or repeat it in an attribute argument. It emits the struct unchanged and
an implementation of a shared marker, `trame::Process`. The actual `.step()` call supplies the
signature and error type to Rust's checker when the process is invoked.

Conceptually, one decorated struct and one arm expand to:

```rust
impl<'worker, 'graph> ::trame::Process for Intake<'worker, 'graph> {}

let mut state = ::trame::__process(intake); // Checks Process + Send; evaluates once.
let mut arm = ::trame::run::arm(move || state.step());
```

The rest is the existing runner. The marker is not a vtable and has no associated `Error`.
There is no generated trait `step` that a missing inherent method could accidentally recurse into.
Rust checks callability, not the textual spelling of the method's receiver: a compatible shared
receiver is also valid. A declaration alone cannot certify its method; a missing method or an
incompatible call fails at invocation. This boundary needs declaration fixtures.

The attribute accepts named, tuple and unit structs, retaining their fields, visibility,
attributes, lifetimes, type/const parameters and bounds. Its generated impl must strip generic
defaults and preserve the where-clause. Enums, unions and functions are outside this declaration.
No field has to be `Copy`; process state has an owner rather than being duplicated to satisfy a
handoff payload bound.

### Invocation and ownership

- Each expression is evaluated once, left to right, before any arm begins. The macro owns an
  ordinary value passed to it. A marker implementation for `&mut P` allows explicit borrowing
  of a declared process when its state must remain available afterwards.
- `step(&mut self)` returns `Result<Step, E>`. `Process + Send` is checked at binding, and the
  existing runner requires one compatible error type across the arms. The process decorator
  adds no new serialization or error-representation promise.
- `Progress`, `Idle` and `Done` keep their meanings. No step follows `Done`, and an idle process
  permits other unfinished processes to advance. Sequential cooperative stepping is valid.
  Boundedness is a caller obligation; warp-uniform application code is not part of the target
  contract. The current nv runner's reliance on it is an implementation constraint to resolve.
- Objects remain alive until the arms have joined or finished. Owned host objects drop on the
  calling side after the join, including when a panic is resumed. A device trap promises no
  Rust destructor execution. Borrowed resources remain their caller's responsibility.

The context form retains the existing tag ownership syntax:

```rust
trame::concurrent!(cx;
    recv(TAG_SYS) => control,
    recv(..) => intake,
    delivery,
)?;
```

Here the selected method is `step(&mut self, io: &mut trame::Io<'_>)`. The macro creates the
existing `arm_io` adapter. An `Io` borrow lasts one step and cannot be retained by safe code. Its
scoped endpoints are the methods `io.{send, lead, recv, flush}`. First-match tag routing, FIFO rules and `NotReceiving` stay unchanged. These are distinct
from worker `trame::{send, recv, flush}` and `leader::{send, recv}`. This form is not a reason to
remove NERVE's separate transport arm during the mechanical migration.

The proposed process-facing macro replaces raw closure arms; it does not guess whether an
identifier holds a closure or a struct. Migrate existing call sites and verification fixtures in
one source-interface change. Closures remain private implementation machinery in the runners.
This avoids a second dispatch grammar, closure-variable ambiguity, and an overlapping blanket
implementation just to keep two public entry styles alive.

## Mechanisms below NERVE

The dependency direction remains NERVE -> trame -> selected backend. No trame module may name
`Bunch`, `Spike`, `SystemMsg`, `Work`, a neuron, or a learning rule.

| NERVE retains | trame owns |
|---|---|
| Worker, graph, model clock and reservoir policy | Process binding, scheduling, joins and execution lowerings |
| Message encoding, revision validity and stop meaning | Opaque byte storage ownership and handoff |
| Synaptic windows and staging ages | Retained transport records and acceptance cursors |
| Learning, firing, wake invalidation and observation meaning | Routing, refusal, publication and collective mechanics |

Extract `src/nerve/pool.rs` by its ownership contract, then extract the generic block/cursor
parts of `bunch/transport.rs`. Keep the current three arms while doing this. The current transport
also decodes stop messages and revisions; moving it wholesale would reverse the dependency.

An owned block is in exactly one place: spare storage, its producer, the queue, or its consumer.
A refused handoff returns that same block. A transport cursor advances only after acceptance.
Pool indices, partial-block offsets and return bookkeeping should not appear in the neuronal
code. Backend storage may differ, but missing capacity remains an explicit refusal.

Do not publish `Vec<u8>` as a portable device-buffer contract merely because the CPU pool uses
one. Device allocation and block ownership must satisfy the same observable capacity and
lifetime rules before the extracted family is public. `NoUninit` word payloads do not make an
arbitrary Rust owner safe to duplicate across lanes. This storage work is a separate gate, not a
precondition for the small process-declaration change.

## The integration boundary

The target is an ordinary declared function over target-owned state:

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

This is not the current `deliver` with an attribute added. The frame's target indexes its neuron,
which is the item's only mutable state. `Integration` is shared: read-only inputs plus the shared
primitives that carry results out, not the entire worker. A key names a dependency domain, not a
promise that every other effect is independent.

### NV lowering

The nv HEAD-surface port uses an inline list-order loop for both item declarations and a
round-robin bounded-step scheduler for struct processes. The shared context is `&C: Sync`;
items, slots and errors keep their documented Send bounds. Every item executes once per call,
including items after a failure. The caller retains only the first list-position error; later
errors are dropped rather than copied or replayed. Repeated keys borrow their one slot in list
order. Infallible bodies use the same loop with `Infallible`.

`sync::with` returns the callback's owned result directly. Handoff stores moved values in
initialized ready/spare slots, returns the unaccepted value on refusal, and drops retained slots
once. Partial construction records initialized spares so host unwind reclaims them. Neither
primitive requires `NoUninit` or copies words between lanes. Their atomics still use nv's own
implementation; no host backend mechanism is imported.

This checkpoint targets the nv host model, not a CUDA certificate. Device entry still assigns a
warp identity and enters a barrier before selecting one application owner; peer and leader
transport still require full-warp convergence. Inline runners and owned primitives must be
called once by the logical owner, not once per physical lane. W2 must establish that owner before
entry and match transport, barrier and atomic address domains to it. The port does not establish
that device prerequisite, resident launch admission, production W1 receive lowering, or W5 device
ownership. Scalar execution is the primary lowering, not a fallback or an acceleration claim.

### Required ownership changes

Current slots are `Vec<Exclusive<Slot>>`, shared with intake queries. They cannot be borrowed as
an exclusive `Keyed` slice through `&Bunch`. Changing nv's runner and shared primitives does not
create that exclusive borrow. Establish target ownership before calling the kernel; do not manufacture `&mut Slot` from a
shared borrow. Queries must obey that ownership cut, rather than read a partially published
parallel transition.

Learning `pre`/`post`, sequence allocation, and wake generations belong to the same target's
ordered history. The present worker-wide `Box<dyn Learning>` does not expose that ownership.
It must be separated by target, or remain outside the parallel region without changing the
relative history of a target. Resolve neither dependency by sorting or dropping frames.

A kernel result must retain original item position and crossing order. Reservoir updates,
fanout, lane staging and logs are published by their owner under an explicit ordering rule.
Invalid targets keep NERVE's reported-drop policy; substituting `Keyed::OutOfRange` as a fatal
worker error would be a semantic change. AlphaLif can produce an unbounded number of crossings
per advance: a fixed result array that truncates them is not an implementation.

### Admission and backpressure

Today a pass retains a bounded extracted list, but `transport::drain` gates each frame before
its neuron changes. A blocked handoff stops subsequent integration. `invoke!`, in contrast,
finishes every item and has no suspend-and-resume result. These are different contracts.

The first process migration preserves that per-frame admission point, current failure behavior,
reset cut and incomplete-pass accounting. It does **not** replace the loop with whole-pass
`invoke!`. A later bulk kernel needs an explicit rule for admitted work, result storage and
publication. No hidden unbounded effect queue, implicit retry of a mutated item, or
integrate-everything-then-send transformation is authorized here.

The invariant for that later change is: every admitted frame is accounted for exactly once;
same-target history is preserved; no unaccepted output is reported accepted; reset distinguishes
published effects from work it discards. Whether admission may move from one frame to a batch
is an application contract decision, still open. Cross-target ordering and state-query cuts must
be stated with that decision. This is a prerequisite to a full NERVE GPU kernel, not a detail a
macro can infer.

### Numerical boundary

Frame deadlines and integration instants are `f64` model time. Resolved weights and neuron
membranes are `f32` where the model defines them; LIF currently computes its exponential in
`f64` before converting the decay to `f32`. Process cleanup changes none of these boundaries,
clock sampling points, thresholds, reset rules or arithmetic order.

A SIMD or CUDA kernel needs a separate numerical claim. Replacing the exponential, contracting
operations into FMA, changing precision or accepting a different threshold decision is not
covered by a scheduling refactor. Use recorded, reproducible inputs and compare state, crossings,
sequence numbers and errors, not just final classification. No speedup is claimed by this design.

## Implementation sequence

| Gate | Work | What must remain unchanged before proceeding |
|---|---|---|
| 0. Contract definition | Separate logical semantics from physical lowerings; record the existing implementation baseline | No hardware mapping or broadcast representation becomes an application obligation. |
| 1. Process declaration | Add the struct marker/attribute and adapt `concurrent!`; migrate its fixtures and callers | Same runner, arm order, outcomes, panic/join behavior and route semantics. |
| 2. NERVE process objects | Move closure-owned state into Intake, Delivery and Transport; keep ordinary step methods | Frame order, admission, clock sampling, reset accounting, logs and wire bytes. |
| 3. Backend storage | Extract generic block ownership and accepted-record cursors | No NERVE names below the boundary; CPU and device storage obey one contract. |
| 4. Kernel ownership | Separate target-owned transitions from publication; decide admission/query cuts | No unsafe keyed aliasing, stale-wake resurrection, changed learning history or clipped crossings. |
| 5. Parallel invocation | Use the existing declarations on the established kernel | Declared error/order semantics hold on hardware; numerical differences are explicitly assessed. |

Gate 1 needs declaration fixtures for generic/borrowed processes, missing or wrong `step`,
non-Send state, error-type disagreement, and short-lived `Io` borrows. Runtime checks must witness
constructor-once behavior, persistent state, no step after `Done`, bounded refusal without replay,
and the existing failure/panic rules. Migrate the old falsifiers rather than replacing them with
weaker ones. Gate 2 compares the scalar event trace for fixed inputs before any parallel change.

Implementation verification proceeds alongside this sequence, not as a prerequisite for defining
the abstract machine. Repair the verifier and M5, and test candidate NV lowerings independently.
A backend must meet the stated contract before its implementation is certified; a working scalar
lowering need not wait for a faster one. The Sol-supervised NV campaign uses an isolated source
snapshot so exploratory code cannot silently become the public implementation.

The NERVE delivery and integration documents describe machinery absent from the code, including
an invoked integration pass and old slot storage. Reconcile them with Gate 2 rather than silently
restoring a former behavior. No new async runtime, backend-specific NERVE module tree, or
performance-tuned constant is part of these first gates.

### P1 surface verifier evidence — W4 Sheets 1–2

`bash trame/scripts/surface.sh /home/pkd/code/agents/nerve-nv-20260929/target-W4/p1` ran with
rustc/rustdoc 1.96.0-nightly (`55e86c996`) and isolated targets under that output. The five
`all.html` inventories each contained 77 exported entries; nv, mpi, rma and rma-lossy each had an
empty diff against none. The 45 expected-refusal fixtures passed, with structured rustc codes and
primary spans at the marked obligation. The one comprehensive positive client fixture failed on
all five selections; the verifier therefore exited 1 (five positive failures, 45 negative passes),
not a portable-contract pass. Its diagnostics include absent `io::*`, `leader::{send_to,recv_from,done}`
and free `sync::{with,handoff::*}` APIs, plus `#[parallel]` refusing the documented shared `&C`
context. Full command records, compiler identity, per-feature inventories, diagnostics and statuses
are in the supplied output directory; the final run log is `/home/pkd/code/agents/nerve-nv-20260929/W4-s12-final3.log`.

The earlier A3 P1 Freeze-only display difference remains recorded above and in
`/home/pkd/code/agents/nerve-addr-20260928/A3-conform.log`; this verifier does not compare rustdoc
synthetic implementation lists. That difference is excluded because `backend.md` states incidental
compiler-generated traits are not contract promises. Its exclusion is not a P1 pass: the client
witness failures above remain red.


nv selects lane zero before worker entry. Other lanes are not application callers;
`init` refuses them before touching a barrier. The bounded cohort occupies one
block, with one physical warp slot per owner. Rings and generation barriers are
scalar acquire/release operations on launch-owned global memory, not full-mask
collectives. Caller-local buffers and owned invocation outcomes stay with the
owner. Cross-owner sync backing and CPU-mapped leader admission still need their
own device evidence; host simulation does not establish address-space legality.


Plain receive's private Owner state is `All`, not a static slice of receive
settings: cuda-oxide cannot lower the latter aggregate fat-pointer constant.
Scoped receive still borrows the ordered arms and retains first-match ownership.
The production lowering witness calls the same public receive and Io paths;
its evidence and compiler-overlay limitations live in the lead report.
