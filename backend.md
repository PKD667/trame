# trame contract

Trame runs logical units that can communicate and share memory. A unit owns its state and advances
asynchronously with respect to other units. A sharing domain identifies units that can access the
same storage under the synchronization rules below. Neither a unit nor a domain names a physical
thread, process, warp, block or machine.

This document describes one host: a leader, the workers it serves and the storage they share. One
thing is wider. An address may name a worker on another host of the same launch, and a frame sent
to it crosses on a link. Which workers exist and where each runs are the launch's decisions, not
this contract's.

The public interface is functions over explicit handles, the four endpoint methods of the
per-step `Io` borrow, execution macros and declaration attributes. A build selects one
implementation. There is no backend trait for applications to implement, scheduler object to
manage, or run-time backend registry.

**This is the target contract, not a certificate that every implementation satisfies it.**
The CPU backends (none, mpi, rma, rma-lossy) implement `#[process]`, the owned shared primitives and
shared-context threaded invocation; the leader endpoint functions and `nv` are not yet migrated.
[execution.md](execution.md) records the migration and existing failures; those failures remain
failures of the version that was tested.

## Definition and implementation

The machine requires two capabilities: independently advancing units that exchange data, and
storage shared within declared domains. The rest of this document defines the semantics trame
builds from them. FIFO routes, bounded handoffs and execution declarations are library guarantees,
not requirements for matching hardware instructions.

An implementation may serialize work, copy bytes, poll a transport or use physical parallelism.
It must preserve ownership, publication, declared ordering and outcomes. `#[parallel]` permits
parallel execution; it does not demand it. `concurrent!` permits cooperative stepping on one
execution resource; it does not demand simultaneous execution. These are valid lowerings, not
error-recovery paths that silently change a failed operation's meaning.

The Rust surface has the same documented functions, macros, attributes, value fields and bounds
on every selection. Private layouts and incidental compiler-generated traits are not the
contract. Resource limits are explicit, and an impossible launch or geometry is refused before
work uses it. A compiler limitation or a failed hardware experiment restricts the implementation
we can claim to support; it does not redefine logical execution.

## Identities and entry

```rust
struct Launch(u32); // Launch::new, get
struct Tag(u16);    // Tag::new, get

fn Deployment::new(hosts: &[&[Launch]], here: u16, leader: Launch)
    -> Result<Deployment, Invalid>;

fn init(env: Environment, deployment: Deployment) -> Result<Context, Failure>;
fn rank(cx: &Context) -> u32;
fn size(cx: &Context) -> u32;
fn done<A>(cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>>;
```

A worker's rank is its dense index in `[0, size)` within its own deployment, a plain `u32`. A
`Launch` identifies a participant in the launch description. They are distinct even when their
integer values happen to agree. Every `u16` is a valid `Tag`. Constructors and inspectors on value
types are ordinary Rust methods; they do not start work.

`Deployment::new` takes the launch's table: `hosts[h][r]` is the `Launch` of host `h`'s worker `r`,
and every process of a launch is given the same table. `here` is this process's host and `leader`
is that host's one leader. Row `here`, position `r`, assigns local rank `r`. It rejects an empty
table or an empty row, a table too large to number, a `here` outside the table, a `Launch` listed
twice, and a leader listed in the table. `init` additionally rejects a participant outside the
launch, inconsistent entry information or insufficient storage. It must not silently narrow a
deployment.

The MPI backends also require each deployment’s leader and workers to share one physical shared-memory domain; all launch participants agree on admission before any bridge is created.

`Environment` is an opaque entry description with `Default`. `Context` owns the entered unit's
backend resources. `rank` and `size` describe this deployment's workers only.

`Context` and `Io` are `Send`, not `Sync`: an owner may move them but cannot create concurrent
shared access to one handle. `Shared`, `Published` and `Leader` are neither `Send` nor `Sync`; their views and
operations retain the owning participant's lifetime. This does not prevent a safely borrowed byte
slice from being shared where Rust permits it. Unstable compiler marker traits are not promises.

`done` is the explicit finalization boundary. On normal completion it returns the supplied
outcome and ends use of the context; no further route or sharing operations use that context.
Finalization is not a delivery barrier: accepted but unreceived frames may be lost, and completion
must not require the application to receive them. An unrecoverable finalization failure is
reported as abnormal termination, not a fabricated successful outcome. There is no promise to
run destructors after a trap or killed process.

## Communication

```rust
enum Channel { Message(Tag), Lane }

enum Error {
    Full, Busy, Closed,
    TooLarge { limit: usize },
    TooSmall { needed: usize },
    Invalid(Invalid),
    Failed(Failure),
}

fn send(cx: &mut Context, to: Addr, channel: Channel, data: &[u8]) -> Result<(), Error>;
fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error>;
fn flush(cx: &mut Context) -> Result<(), Error>;
```

Lengths, offsets and capacities are bytes, represented by `usize` at the Rust boundary. A frame
is opaque bytes, not an implicitly serialized Rust value. `Frame::source`, `tag` and `len` describe
the whole frame written into the caller's buffer. No partial frame is reported as a receive.

Each call makes one attempt. `Full` means no capacity, `Busy` means contention at that attempt,
and `Ok(None)` means no eligible frame was received. Refusal accepts or consumes nothing.
`TooSmall { needed }` leaves the frame available for a buffer with `needed` bytes. The caller
may attempt again, perform other work or stop; the backend does not hide a waiting loop inside
a refused operation. An observed backend failure is not a transient refusal.

Successful `send` accepts a private copy of the bytes before returning. The caller may reuse
its buffer immediately. Successful `flush` means accepted sends retain no send-side storage;
it does not mean a receiver consumed them. Publication of an accepted frame precedes its
successful receive, including visibility of every payload byte.

`Message` is reliable FIFO per sender/receiver pair across its tags while the route is live.
`Lane` is FIFO per pair; only an explicitly lossy lane may skip frames, and it never reorders
those delivered. Nothing orders a Message frame against a Lane frame. Closing a route or ending
a participant does not imply delivery of its pending frames.

A live route must not permanently refuse admissible traffic merely because its implementation
never services pending work. Progress assumes a live launch, execution opportunities for the
participating units, and receivers that perform the required receives. There is no wall-clock
latency bound or promise of delivery to an application that never receives.

### Leaders

A leader is an external communication participant without a worker rank. It is an endpoint role,
not a requirement for a dedicated operating-system process. A deployment has exactly one leader,
and every worker of it answers to that leader.

```rust
fn leader::open(env: Environment, deployment: Deployment) -> Result<Leader, Failure>;
fn leader::send_to(leader: &Leader, to: u32, tag: Tag, data: &[u8]) -> Result<(), Error>;
fn leader::recv_from(leader: &Leader, out: &mut [u8]) -> Result<Option<Frame>, Error>;
fn leader::done<A>(leader: &mut Leader, outcome: Result<(), Failure<A>>)
    -> Result<(), Failure<A>>;

fn leader::send(cx: &mut Context, tag: Tag, data: &[u8]) -> Result<(), Error>;
fn leader::recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error>;
```

The send and receive functions have the same one-attempt and whole-frame rules. Each direction
is FIFO per worker. A frame received by a worker from its leader has `source == None`; the leader
receives `Some(Local(r))`, the sending worker's rank. `open`, `send_to` and `recv_from` replace the currently
implemented `Leader::open`, `send` and `recv`; no second interface is intended.

`leader::done` gives the leader the same explicit finalization boundary as a worker. It is a
proposed addition: the current MPI implementation instead coordinates shutdown in `Leader`'s
Drop. Normal programs finalize entered participants explicitly; dropping a handle is not an
implicit successful finalization or a new collective participation point.

### Declared lanes

```rust
fn reshape(cx: &mut Context, workers: &[u32], edges: &[Edge], frame: usize, tag: Tag)
    -> Result<(), Error>;
fn release(cx: &mut Context) -> Result<(), Error>;
```

`reshape` declares directed routes: ascending unique workers, edges ascending by
`(source, destination)`, at least one endpoint `Local` and every `Local` endpoint among the
workers, maximum frame length in bytes, and a
tag. `Edge::new(source, destination, affected)` records a nonzero destination-element count used
for capacity planning. Storage geometry is an implementation decision, not an application-visible
ring formula. Unsupported geometry is refused before any lane send.

Traffic before configuration is `Invalid(LaneNotConfigured)`. An undeclared pair is
`Invalid(NoLane)`, and exceeding the declared length is `TooLarge { limit: frame }`. A lane frame
uses the declared tag. `release` ends this participant's use of the lanes; it does not certify
that peers consumed pending frames.

### Addresses

```rust
enum Addr { Local(u32), Remote { host: u16, rank: u32 } }
```

A worker has to name its peers, and the name should tell trame how to reach them. On your host a
worker is its rank, `Local(r)` with `r < size`. On another host of your launch it is
`Remote { host, rank }`: the launch's number for that host and the worker's rank there. A worker
on your own host is always `Local`, and a `Remote` naming your host is refused, so from where you
stand a worker has exactly one address. The launch fixes both numberings at entry, and they do
not change while the deployment lives. An address that names no worker of the launch, a `Remote`
naming your own host included, is `Invalid(RankOutsideJob)`.

Only peer routes use an address: `send`, `Io::send`, `Edge` and `Frame::source`. What concerns
your host alone takes a plain `u32`: `rank`, `reshape`'s workers and `leader::send_to`. A remote
worker cannot reach those, by type.

### Links

A frame to a `Remote` worker crosses hosts, and you do not choose how. The backend lowers it onto
a link the launch established, beneath `Message` and `Lane`. You get the same channel, the same
one-attempt outcomes and the same FIFO per directed pair. A link never loses a frame on any
backend, even where `LOSSY` lets a local lane skip. `none` refuses a remote address by name, and
`nv` reports `Unimplemented`. `reshape`, `release` and `barrier` never cross a host, and an
accepted frame is not proof that the remote worker received it.

## Shared memory

```rust
struct Handle; // Handle::BYTES, to_bytes, from_bytes

fn leader::publish(leader: &Leader, revision: NonZeroU64, bytes: &[u8]) -> Result<Published, Error>;
fn leader::handle(segment: &Published) -> Handle;
fn leader::retire(leader: &Leader, segment: Published) -> Result<(), (Published, Error)>;

unsafe fn attach(cx: &mut Context, handle: Handle) -> Result<Shared, Error>;
fn bytes(segment: &Shared) -> &[u8];
fn detach(cx: &mut Context, segment: Shared) -> Result<(), (Shared, Error)>;
```

A segment belongs to a leader. `publish` allocates storage for `bytes`, writes every byte, and
then publishes it under `revision`; the leader is the segment's only writer and nothing writes it
after publication. `handle` names it as a fixed-size value, `Handle::BYTES` long, that the
application carries to its workers in an ordinary frame; the segment's bytes never travel as
frames, so its length is not bounded by `MAX_FRAME`. A `Handle` holds the revision, the length and
a backend-private token. Decoding accepts any bytes except revision zero; whether they name a
segment is decided by `attach`.

`attach` maps the named segment read-only. It acquires the segment's published revision before it
reads any other header field or payload byte. A header disagreement in revision, length or format
is `Invalid(NoSegment)`; an OS failure opening or mapping the name, including ENOENT when it was
retired or its leader is not colocated, is `Failed(BackendFault::Os(errno))`.
It is `unsafe` because the caller promises that the segment is not retired before the returned
`Shared` is detached or dropped; on some storage retirement frees memory a live view reads. The
`Shared` owns the mapping and `bytes` borrows it. `detach` consumes it on success and returns the
same live handle and precise OS error on refusal, and so does `retire` for a `Published`. A
recorded cleanup failure is sticky: the next `detach` or `retire` returns the same errno without
issuing a syscall, and `retire` is the owner's alone, asserting on a reader. Cleanup in Drop is
abnormal: an OS failure there is reported and aborts instead of panicking or disappearing.
Dropping a returned refusal without handling it reports the original error and aborts, never retries.

A leader may publish revision `n + 1` while `n` is attached, so a refused publication leaves the
previous segment and its readers untouched. On named storage the MPI family refuses a revision the
leader already holds live by name (`EEXIST`); `none` does not check it and relies on the leader
never reusing a live revision. Retiring is the
leader's decision alone, taken after every attached worker has told it, by an ordinary frame, that
it has detached.

Publish reserves the whole object before it copies, so one live segment needs its length in
`/dev/shm`, and a successor published beside it needs both lengths at once. An exhausted `/dev/shm` is a refused
publication carrying its errno, such as `ENOSPC`, not a `SIGBUS` during the copy.

A process that dies before retiring its segments, whether by a fatal exit, a signal or a launcher abort,
leaves its `/dev/shm/trame-<pid>-<rev>` objects behind. Removing them after a crash is the launch
operator's job; there is no automatic crash cleanup yet.

**Colocation is a launch precondition.** A leader runs on its workers' machine, in their POSIX
shared-memory namespace (host backends) or their CUDA context (device backend). Nothing discovers
or repairs a violation; `attach` refuses by name.

Shared mutable state uses exclusive access, handoff or atomics, not an aliased `&mut` reference.
Shared storage has no implicit global coherence outside its declared domain. Logical publication
and acquisition must establish visibility regardless of which memory instructions implement them.

A segment carries bytes from a leader to its workers. The Rust-value primitives below serve
processes sharing one worker's valid address domain; they do not serialize arbitrary `T` into
another worker's address space. Neither `Send` nor `NoUninit` makes a pointer valid in a different
address domain. A lowering must keep these borrowed values accessible to their logical owners.

### Exclusive access

```rust
fn sync::with<T, R>(state: &Exclusive<T>, body: impl FnOnce(&mut T) -> R)
    -> Result<R, Locked>;
```

`Exclusive::new(value)` creates the owned state; `Exclusive<T>` is `Sync` when `T: Send`.
`Locked` distinguishes `Busy` from `Abandoned`. `with` makes one acquisition attempt. On success
it calls `body` exactly once with the only mutable borrow and returns its result to the logical
caller. Release publishes the mutation to the next successful acquisition. Contention is
`Locked::Busy`, not a hidden wait for another process step.

If an unwinding panic escapes the body, later acquisitions report `Locked::Abandoned`; they do
not silently treat partially mutated state as sound. A trap ends the launch instead. The return
value `R` need not be a padding-free copy value. Duplicating it among physical execution lanes is
not part of this operation. The current `Exclusive::with` method is the implementation to migrate.

### Handoff

`sync::handoff` supplies the following functions over `Handoff<T, D>`, `Sender` and `Receiver`:

```rust
fn new<T, const D: usize>(init: impl FnMut() -> T) -> Handoff<T, D>;
fn split<T, const D: usize>(queue: &mut Handoff<T, D>)
    -> (Sender<'_, T, D>, Receiver<'_, T, D>);
fn spare<T, const D: usize>(sender: &mut Sender<'_, T, D>) -> Result<T, Idle>;
fn send<T, const D: usize>(sender: &mut Sender<'_, T, D>, value: T) -> Result<(), Unsent<T>>;
fn recv<T, const D: usize>(receiver: &mut Receiver<'_, T, D>) -> Result<T, Idle>;
fn give<T, const D: usize>(receiver: &mut Receiver<'_, T, D>, value: T) -> Result<(), Unsent<T>>;
fn sending<T, const D: usize>(receiver: &Receiver<'_, T, D>) -> bool;
```

`D` is a positive item count. Construction creates `D` spare values; the ready queue also has
capacity `D`. One producer and one consumer borrow the handoff. Each operation attempts once.
`send` publishes the moved value and `recv` acquires the oldest queued value. `give` returns a
value to the spare pool. Publication includes the data the transferred ownership permits access to.

`Unsent<T> { why, value }` returns the same unaccepted value with `Full`, `Busy` or `Closed`.
An empty `spare` or `recv` reports `Empty`, `Busy` or `Closed` through `Idle`. Closing the sender
does not erase queued values; the receiver can drain them. `sending` reports whether the sender
still exists. Once both endpoints are gone the handoff may be split again.

At each transfer an owned value is in one location: its caller, a spare slot or a ready slot.
Ownership is moved, never duplicated. Reclamation drops retained values exactly once; dropping
an endpoint does not fabricate acceptance of its outstanding work. `T: Send` is required when
endpoints move between execution threads, through their Rust trait bounds, not because a wire
serializes `T`. No `Copy` or `NoUninit` requirement follows from the logical ownership contract.
These function forms replace the corresponding current constructor and endpoint methods.

### Atomics

`sync::atomic` provides `AtomicBool`, `AtomicU32`, `AtomicU64` and `Ordering`. The value widths
are one Boolean, `u32` and `u64`, not unspecified machine words. Operations are `new`, `load`,
`store`, `swap`, `compare_exchange`, `Default`, and integer `fetch_add`, with Rust's ordering
semantics within the sharing domain. An implementation may serialize atomic operations; it may
not omit the visibility an acquire/release pair promises.

## Coordination boundaries

Participants enter matching coordination operations in matching order:

```rust
fn barrier(cx: &mut Context);
```

| Boundary | Participants |
|---|---|
| Entry | Each declared worker calls `init`; each deployment's leader calls `leader::open`. |
| `reshape`, `release` | All entered workers, never leaders; `workers` in `reshape` describes the active route geometry, not a different collective membership. |
| `barrier` | All entered workers, never leaders; it consumes no message or lane frame and does not finalize sends. |
| `publish`, `retire` | The leader alone; `retire` only after each worker that attached has reported its `detach`. `attach` and `detach` are each one worker's own and coordinate with nobody. |
| Finalization | Entered workers call `done`; entered leaders call `leader::done`. Releasing their joint resources may coordinate their completion, but never requires application delivery. |

`barrier` separates application phases but does not certify delivery of pending frames: callers
must finish their previous exchanges before entering.

Route declarations agree among participants. Segment publication is not a collective: the leader
publishes, and the handle reaches each worker as an application frame. The implementation must
schedule all admitted participants needed by a boundary; physical simultaneous residency is not a
caller obligation.

These boundaries do not run inside a bounded process step or an invoked item. Such bodies must
finish without requiring another body to be scheduled while they wait. Coordination has no
intrinsic wall-clock timeout; the launcher owns termination of a stalled or failed launch.
Resource impossibility is an explicit entry/configuration error, not a collective that waits
for a unit the backend can never run.

## Execution declarations

### Persistent processes

```rust
#[trame::process]
struct Intake { /* application-owned state */ }

impl Intake {
    fn step(&mut self) -> Result<trame::Step, WorkError> { /* bounded body */ }
}

trame::concurrent!(intake, delivery, transport)?;
```

The struct attribute declares retained work, not a new object hierarchy. It leaves fields and
ordinary inherent methods intact. Its marker lets `concurrent!` reject undeclared objects; Rust
checks the generated `.step()` call and infers its error type at invocation. No associated-error
annotation or generated public factory closure is needed.

`concurrent!` binds each expression once in source order before stepping any of them. Values are
moved unless the caller explicitly passes a mutable borrow. The processes must be `Send`, may
borrow scoped state, and return one compatible `E: Send`. Each process retains its state across
calls and is never stepped simultaneously with itself.

`Step::Progress` means work advanced; `Idle` means this attempt did not advance; `Done` permanently
finishes that process. An idle process must permit other unfinished processes to run. While the
launch remains live, unfinished processes get repeated execution opportunities; there is no
promise of an exact interleaving, thread count, polling frequency or completion time.

A sequential scheduler repeatedly gives each unfinished process a bounded step. Running one
process to completion before admitting its communicating peer is not an equivalent lowering.
Neither process bodies nor callers must name lanes or perform warp votes.

An error stops admission of new steps. Already admitted steps may finish. After joining all
admitted work, the first failing process in source order among the recorded errors supplies the
result. This does not promise the same set of errors under every interleaving. A host panic is
joined with the other work and then resumed; a device trap is reported by the launcher. Owners
and borrowed resources stay alive until admitted work has ended.

### Per-process communication

```rust
trame::concurrent!(cx;
    recv(TAG_SYS) => control,
    recv(..) => intake,
    delivery,
)?;

impl Io<'_> {
    fn send(&mut self, to: Addr, channel: Channel, data: &[u8]) -> Result<(), Error>;
    fn lead(&mut self, tag: Tag, data: &[u8]) -> Result<(), Error>;
    fn recv(&mut self, out: &mut [u8]) -> Result<Option<Frame>, Error>;
    fn flush(&mut self) -> Result<(), Error>;
}
```

In this form each step is callable as `step(&mut self, io: &mut Io<'_>)`. The endpoint borrow
lasts one step. `Io` is the one handle with methods: it exists only to carry these four
operations for one step, and `io.send` against `trame::send(cx, ..)` already names the scope.
They take the same routes as the worker operations; `lead` sends to the worker's assigned leader.

`recv(A, B)` assigns those tags; `recv(..)` assigns every tag. A frame belongs to the first process
in source order whose setting names its tag, whether it came from a peer, lane or leader. Frames
no process names remain at the backend. A process without a setting gets `Invalid(NotReceiving)`
if it tries to receive.

Frames from one sending process to one receiving process retain per-channel FIFO across its tags.
Frames assigned to different processes have no mutual order, and a frame may wait behind an
earlier frame of the same sender owned by another process. Tag dispatch does not authorize
silently dropping or reordering traffic.

### Item invocation

```rust
#[trame::parallel]
fn advance(item: Item, cx: &Pass) -> Result<(), WorkError> { /* bounded body */ }

#[trame::parallel]
#[trame::ordered(key = frame.target: Target)]
fn integrate(frame: Frame, slot: &mut State, cx: &Pass) -> Result<(), WorkError> {
    /* target-owned transition */
}

trame::invoke!(advance, &pass, &items)?;
trame::invoke!(integrate, &pass, &frames, trame::Keyed::new(&mut states[..]))?;
```

`#[parallel]` dispatches a list. `#[ordered]` says which mutable state each item may touch: its
key indexes one slot of a `Keyed` slice, so a frame reaches the slot its target names and no
other. That slot is the only mutable borrow an item receives. The context is shared, `&C` with
`C: Sync`, and carries read-only inputs and the shared primitives below. Mutation outside the
keyed slot goes through those primitives, never through an aliased `&mut`. An optional `&self`
receiver is shared in the same way.

Items come from `&[I]` with `I: Copy + Send`: each body receives an item by value from that
borrowed list, and a copied item may be handed to an execution thread other than the caller's.
`Copy` alone does not allow that (a copied `&Cell<u32>` is `Copy` and not `Send`), and `Sync` is
not needed, since an item is moved once and never shared. A key is `Copy + Into<usize>`; its type
distinguishes keyed views. Slots are `T: Send` and errors `E: Send`, for the same reason. These bounds describe the Rust call, not a device transport representation.

`invoke!` visits every item once and joins the work before returning. An item error does not
cancel later items. A keyed item outside the slice records `Invoked::OutOfRange { key, len }`
without entering its body; an in-range body's failure is `Invoked::Failed(E)`. The first failing
item in original list order determines the returned error, including out-of-range failures.
Empty input succeeds without calling a body. An item that returns nothing cannot fail: its `E` is
`Infallible`, and only an out-of-range key can be the error of a keyed call. It is written that way
so a body whose effect is all it does need not invent a `Result` to say so. A panic is not an item `Err`: before unwinding,
a host lowering must join all work already admitted, but need not admit remaining items. A device
trap is reported by the launcher, with no completion or destructor guarantee.

Repeated keys are legal. Calls for one key retain input-list order and never overlap their
mutable access to its slot. No additional cross-key execution order is promised. A sequential
list-order loop is a complete implementation of both declarations.

Distinct keys may run in parallel; the same key never overlaps itself. An implementation must
not copy the context per lane, duplicate an effect, or replay an item to recover its error. A
declaration does not waive Rust's ownership rules or make a collective safe in an item body.

There is one logical result owner. `E` need not implement `Copy`, `Clone`, `NoUninit` or a device
serialization trait. The retained error moves to that owner; on normal completion, other errors
are dropped exactly once before return. Their destruction order is not otherwise specified.
Moving an error to its owner needs `E: Send`, not a copy; a physical implementation's desire to
broadcast it cannot strengthen the public bound. Different arithmetic precision or reordered same-target updates are not scheduling choices.

## Failure and clocks

```rust
struct Failure<A = Infallible> {
    participant: Participant,
    operation: &'static str,
    kind: FailureKind<A>,
}
enum FailureKind<A> { Backend(BackendFault), Application(A) }
enum Participant { Worker(u32), Leader(Launch), Entering(Option<Launch>) }

fn clock::reading() -> Reading;
```

A backend-observed failure returns as a value identifying its participant and operation. Failure
is not rollback: effects already accepted remain effects, and work is not automatically replayed.
Failed receive is `Error::Failed`, never fabricated data. Entry failure uses `Entering` rather than
inventing a worker rank. A trap or lost participant cannot be required to create a record; the
launcher reports abnormal termination. Application errors remain distinct from transport faults.

A `Reading` is an integer `Span` in `u64` nanoseconds from an origin, tagged with `ClockId`.
`Reading::since` refuses different identities with `ClockMismatch`. Clocks are not globally
synchronized, and scheduling macros do not sample or substitute application model time.

## Build description and evidence

`ID: Backend` identifies the selected implementation for diagnostics and persisted records;
existing `wire_id` values are append-only. `LOSSY: bool` licenses Lane overwrites and nothing
else. `MAX_FRAME: usize` bounds every route, including the leader route. The present library
profile requires at least 65,544 bytes and at most `u32::MAX`; a backend must provision that
profile or refuse entry. This numerical profile is not a hardware definition.

`none` is the degenerate deployment: one worker and no functioning transport peer. It refuses
other worker geometries, sends answer `Closed`, and receives produce no frame. Local execution and
shared-state primitives still work, and its leader's segment is a process-local copy that the
worker, in the same process, attaches by address; `attach` there cannot validate the handle and
relies on its `unsafe` promise. It does not simulate evidence for communicating deployments.

`nv` implements the segment surface as refusals: `leader::publish` and `attach` return `Failed`
with `BackendFault::Unimplemented`, so no `Published` or `Shared` value exists on it and `bytes`,
`detach` and `retire` cannot be reached. `Published` is `nv`'s own uninhabited type, distinct from
the worker's `Shared`, not an alias of it. The intended implementation is a leader device
allocation filled by host-to-device copy on a pre-created independent stream, with the handle's
token the device pointer, valid in the launch's single CUDA context; one GPU per leader. cuda-core
allocation and its freeing `Drop` are synchronous, so whether they make progress beside a
persistent kernel is an open hardware question, not a settled lowering. In S1 conformance `nv`
checks that `publish` and `attach` refuse with `Unimplemented`; the table reports `UNIMPLEMENTED`
only when the leader's `publish` refusal and each of the four workers' `attach` refusals is the
exact `Unimplemented` failure, and the cell is not evidence of segment transfer.

Backend build tooling belongs to its implementation: `trame/<module>/cargo`, package
`<module>-cargo`, with Cargo's `cargo-<module>` executable convention. NV therefore has
`trame/nv/cargo`, package `nv-cargo`, invoked as `cargo nv`. A module with no build tool needs no
placeholder crate. Tooling is not part of the worker's execution interface.

Conformance checks observable effects against this contract, not which hardware instruction,
scheduler or storage layout produced them. Verification records the specification version,
source snapshot, compiler, configuration and hardware. A failed implementation is repaired or
left uncertified; a changed contract requires an explicit migration and new evidence. Host
models, compiler probes and hardware runs remain separate kinds of evidence.
