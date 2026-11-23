# The backend contract

A backend runs a launched topology. It implements the trait `Backend` (`backend.rs`), whose
signatures the compiler checks. This file states what the signatures cannot. A backend is accepted
when it keeps every clause below.

Clauses are named. Code, tests and proofs cite them by name: a `SAFETY` comment names the clause
that makes its `unsafe` block sound.

## Terms

A *worker* has a state and steps. Step i *reads* the fields in R(i) and *writes* those in W(i):
`&field` and `&mut field` in its signature. Two steps *conflict* when one writes a field the other
reads or writes. An *agent* is a thread that runs a worker's steps. The *leader* builds states,
sends, receives, edits, stops and joins. A worker's *domain* is the memory in which its state lives
and in which its agents and its leader address it at the same addresses. The *model* is `sched.rs`.

*T* is the transition system that defines trame's semantics: the rules Tick, Now, Send, Full,
Overwrite, Recv, Empty, Take, Release, Drop, Start, End, Parallel, Hold, Edit and Stop, with the
endings Abort and Deadlock. $\mathit{Obs}(B, P, G)$ is the set of runs of program P with topology G
on backend B, each recorded as every agent's sequence of labels and its result.

## Semantics

**R1 Safety.** For every admitted program P and every topology G that B accepts,
$\mathit{Obs}(B, P, G) \subseteq \mathit{Obs}(T, P, G)$.

A backend that does nothing satisfies R1. R2 rules it out.

**R2 Progress.** For every accepted launch, assuming the hardware keeps running and every agent that
runs forever performs effects forever (a backend may switch agents only at effects):
- time diverges;
- every frame becomes available unless a lossy route removes it first;
- Release eventually fires on every route with a slot taken and not yet given back;
- every effect that T can still answer is answered, and every started step reaches End;
- a frame that stays an offer to x on r, while x receives r's tag infinitely often, is eventually
  received;
- if the premises of Start hold for p from some point on, p is eventually started;
- if Hold has held fields, Edit eventually fires for them.

The clauses that follow refine R1 and R2 at the seam. Where a clause and T disagree, one of them is
wrong.

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
change to its worker, which is an end, a frame, room or an act of the leader.

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

**M2 Meaning.** A state, the pool and a step's error hold no value whose meaning depends on the process or device that made
it: no reference, raw pointer, function pointer, trait object, `Rc` or handle.

**M3 Mutation.** A field changes only by a step that writes it, or by an edit. A state holds no
interior mutability.

M2 and M3 are the type `trame::Plain`: every field of a state, the pool and the error type are `Plain`, and a type
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

**F3 Port agreement.** A route between workers of two hosts is declared on both, with the same
ends, message type, kind and capacity. Hosts whose declarations differ are refused at launch (L1),
both of them, with a message naming the route. Only hosts that share routes wait for each other's
launch. A route that one host declares and the other does not is refused on the host that declares
it, at the latest when the other host ends; the other host does not wait.

**F4 Tag.** A message crossing hosts is identified by a tag equal in two compilations of the same
type: a hash of its path and its size. `TypeId` does not cross hosts.

**F5 Routes between hosts.** A route between workers of two hosts keeps T1–T3 while both workers
run. A route between a leader and a worker of another host is refused at launch. A worker that waits
on such a route forever is stuck, and ends with `Deadlock` once every leader waits, joins or has
ended (E2).

**F6 Peer channel.** A peer of a leader is the leader of any other host. Leaders exchange messages, before a launch or after it, over a
peer channel that is not a route. A host is not its own peer, and a host number of H or more names no
host; a send to either panics.

## End

**E1 Abort.** A panic ends the run. Every worker without a result unwinds at its next effect, and
every leader call on the ended run, including one already blocked, panics with
`trame: worker <w> panicked: <message>`, naming the first worker that panicked. The leader observes
no result of an aborted run.

**E2 Join.** `join` returns once every worker has a result, with one result per worker, in the
order the workers were declared. A worker is *stuck* when no run from the present contains a Start of
one of its steps. A result is `Deadlock` only where the backend shows the worker stuck. A leader
call that can never return may be panicked with `trame: deadlock: the leader waits and nothing can
arrive`, under the same condition.

## Latency

T leaves latency to the environment, so no clause of T bounds it. Q states two clauses for a named reference
deployment, and their numbers are set in the lab, not in this file.

**Q1 Median.** On the reference deployment, the median round trip of each placement and message size is within
its bound.

**Q2 Tail.** On the reference deployment, the 99th percentile of round trips of each placement and size is within
its bound.

A backend is accepted only if its latency claims are supported. A bound that is not yet set is absent, and no
clause is claimed for it.
