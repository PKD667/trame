# The backend contract

A backend runs a launched topology. It implements the trait `Backend` (`backend.rs`), whose
signatures the compiler checks. This file states what the signatures cannot. A backend is accepted
when it keeps every clause below. `PROOFS.md` says how each clause is checked and where each check
stops.

Clauses are named. Code, tests and proofs cite them by name: a `SAFETY` comment names the clause
that makes its `unsafe` block sound.

## Terms

A *worker* has a state and steps. Step i *reads* the fields in R(i) and *writes* those in W(i):
`&field` and `&mut field` in its signature. Two steps *conflict* when one writes a field the other
reads or writes. An *agent* is a thread that runs a worker's steps. The *leader* builds states,
sends, receives, edits, stops and joins. A worker's *domain* is the memory in which its state lives
and in which its agents and its leader address it at the same addresses. The *model* is `sched.rs`.

## Semantics

**R1 Safety.** Every run the backend produces is a run of the transition system T (`spec.md`,
Section 5 and Requirement (Safety)).

**R2 Progress.** The backend meets Requirement (Progress) of `spec.md`, Section 6.

The clauses that follow refine R1 and R2 at the seam. Where a clause and `spec.md` disagree, one of
them is wrong.

## Launch

**L1 Refusal.** A launch is refused before any step starts, and on every process of the launch or
on none.

**L2 Agreement.** Every process of a launch runs the same plan. A launch whose processes were given
different plans is refused.

**L3 Building.** Each value given by `Topology::set` is built once, by its builder, in the domain
where it is used, before any step starts.

## Scheduling

**S1 Exclusion.** While step i runs, no step of its worker that conflicts with i runs. `State::run`
is called only under S1.

**S2 Start.** A step starts only when `Sched::may_start` holds for it, with the wait conditions the
backend observed at that start.

**S3 Wake.** A waiting step is woken by the end of a step that writes one of its fields, or by a
condition it named. No wake is lost: an agent that finds nothing to start sleeps only until the next
change to its worker, which is an end, a frame, room, its deadline or an act of the leader.

**S4 Edit.** An edit holds a set of fields: all of a worker's fields, or the pool. From the hold on,
no step touching a held field starts. The edit's function runs once none runs. Then the fields are
given back and every step touching one is woken. Queued frames are kept.

**S5 Stop.** After a stop, no ordinary step starts. The stop step, if the state has one, starts
only when no step runs. The worker's result is `Stopped` once the stop step is done, or at once if
there is none.

**S6 Priority.** A step starts only if no step ranked above it that wants to start conflicts with
it (`Sched::wants`, `Sched::rank`). A step waiting for a field is overtaken by no conflicting step of
lower rank. It waits only for the steps holding the field when it began to wait. A weight-0 step
may wait for ever behind steps of positive weight.

**S7 Shares.** Under contention, steps of positive weight receive service in proportion to their
weights. The backend defines service and states the tolerance it keeps.

**S8 Failure.** When a step fails, its worker's result is `Failed`, no step of the worker starts
again, and the error of the first failing step is kept for the leader.

**S9 Trace.** When the launch asks for a trace, the backend records each worker's Start, End,
Stop, Hold and Edit events, in an order consistent with the run. `sched::check` accepts it.

## Transport

**T1 Order.** Frames on one route are received in the order they were sent.

**T2 Capacity.** A send answers `Full` only on a lossless route at capacity. A send on a full lossy
route replaces the oldest frame.

**T3 Delivery.** A frame is received at most once, and only by its route's receiver. A frame on a
lossless route is eventually available to its receiver.

## Pool

**P1 Writer.** Only the leader writes the pool, in an edit. Steps read it.

**P2 Edit.** A pool edit is S4 over the pool, in every worker at once.

## Memory

**M1 Placement.** Every byte that a state or the pool owns lies in the domain where it is used.
Builders and edits allocate there.

**M2 Meaning.** A state holds no value whose meaning depends on the process or device that made
it: no reference, raw pointer, function pointer, trait object, `Rc` or handle.

**M3 Mutation.** A field changes only by a step that writes it, or by an edit. A state holds no
interior mutability.

M2 and M3 are the type `trame::Plain`: every field of a state, and the pool, is `Plain`, and a type
is `Plain` by trame's own implementations, by `#[trame::message]`, or by `#[derive(trame::Plain)]`,
which requires it of every field. An `unsafe impl Plain` written by hand is the program's promise.

**M4 Allocation.** Steps neither allocate nor free.

**M5 Visibility.** What a builder or an edit wrote is visible to every step that starts after it
returns, and to no step that runs during it.

**M6 Bound.** A domain has a size fixed at launch. An allocation in the domain that does not fit
answers null and allocates nothing outside the domain; the process then stops with Rust's
allocation failure, after the backend has said that the domain is full. A full domain is not a
`Refused`: no program can continue from it.

**M7 Lifetime.** A state and the pool live from the launch until the join, whatever the program
does with its `Worker` and `Pool` handles. A handle dropped after `set` frees nothing a step uses,
and a run dropped without its join leaks its states rather than free them under running workers.

## Federation

**F1 Identity.** Each host of a deployment has a `HostId`, distinct from every other host's and
fixed by deployment; `host()` answers it. Workers are numbered from 0 on each host; a worker of
another host is named by its host and its number there.

**F4 Tag.** A message crossing hosts is identified by a tag equal in two compilations of the same
type: a hash of its path and its size. `TypeId` does not cross hosts.

**F6 Peer channel.** Leaders exchange messages over a peer channel that is not a route. A host is not
its own peer; a send to a host that is not a peer panics.

## End

**E1 Abort.** A panic ends the run as Definition (Abort) of `spec.md` states.

**E2 Join.** `join` returns once every worker has a result, with one result per worker, in the
order the workers were declared. A result is `Deadlock` only where the backend shows the worker
stuck (`spec.md`, Definition (Deadlock)).
