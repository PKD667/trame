# Backend contract

## 1. Scope

NERVE defines model state, operations, ownership and order. A backend defines their execution,
communication, clocks and storage lifetime. Dependencies shall point from NERVE to the backend.
The portable worker shall not name an OS, transport library, allocator or backend implementation.

A **participant** is one independently identified execution unit with a rank in the deployment.
It may be a process, a thread, or a cooperating group of device lanes, and it is a **worker**:
`size`, `rank`, `hosts` and `cohort` describe workers and nothing else. A deployment may also
include a **leader**, which is a host process: it is not a participant, it has no rank, and it
appears in none of those four. A **sharing domain** is a set of participants that can read one
immutable allocation. Neither term implies an OS process or a physical host.

`shall` denotes a requirement. An unsupported requirement shall fail compilation; invalid
run-time inputs shall produce a named error. A backend shall not substitute a weaker guarantee.

## 2. Values and selection

One backend is selected at build time. The surface uses values, not trait objects, and shall
not require an allocator. Storage is supplied at entry or acquired under a declared, bounded
allocation policy.

```rust
struct Rank(u32);                       // private field; `Rank::from_index`, `get`
struct Tag(u16);                        // private field; `Tag::new`, `get`
struct FrameBytes(u32);                 // private field; `TryFrom<usize>`, `get`
struct ByteRange { offset: usize, length: usize }
enum Backend { Mpi, Rma, RmaLossy, None, Nv }   // `wire_id`, `name`
type Environment;                       // entry-supplied identity, resources and lifetime
type Context;                           // participant-local state established by init
struct Deployment<'a>;                  // built only by `Deployment::new`
type Shared;                            // immutable mapping with explicit retirement
struct ClockId;                         // comparison domain, including clock incarnation
struct Failure<A = Infallible> { participant: Rank, operation: &'static str, kind: FailureKind<A> }

enum Channel { Message(Tag), Lane }
struct Frame;                           // `source`, `tag`, `len`
struct Edge;                            // `Edge::new(source, destination, affected: NonZeroU32)`
struct Span;                            // nanoseconds
struct Reading;                         // `clock`, `elapsed`, `since`

const ID: Backend;
const LOSSY: bool;
const MAX_FRAME: FrameBytes;            // >= 65_544 on every backend, checked at compile time
```

Frame lengths and capacities are bytes, as `FrameBytes`; conversion to a narrower transport count
shall be checked. Segment lengths use `usize`, in bytes of the target address space. `Rank` values
are dense in `[0, size)`. A clock identity shall not be inferred from a participant rank.

Every route carries a frame of `MAX_FRAME` bytes: a 64 KiB log block behind an eight-byte batch
header is 65,544 bytes, and every backend shall provision at least that and refuse at entry a
launch whose storage cannot hold it. `Backend::wire_id` is on disk in every `LOAD` record, so its
values only grow at the end.

Signatures describe logical participant borrows, not a per-lane device ABI. Cooperative lowering
shall transform mutable parameters and every access to them together, before overlapping Rust
`&mut` references can exist. Uniform or partitioned access is determined per access, not per value.
Borrowed values shall not escape to unlowered callees; native adapters shall specify their
ownership and convergence obligations. Unsupported ownership shapes shall fail compilation.

There are no capability declarations. Every public operation has one meaning under every backend,
so a caller cannot select behaviour per backend and has nothing to require at build time. What
differs between backends is only `ID`, `LOSSY` and `MAX_FRAME`.

## 3. Entry and failure

```rust
fn init(env: Environment, deployment: Deployment<'_>, cohort: fn(Rank, &[Rank]) -> u32)
    -> Result<Context, Failure>;
fn Leader::open(env: Environment, deployment: Deployment<'_>) -> Result<Leader, Failure>;
fn rank(cx: &Context) -> Rank;
fn size(cx: &Context) -> u32;
fn hosts(cx: &Context) -> &[Rank];
fn done<A>(cx: &mut Context, outcome: Result<(), Failure<A>>) -> Result<(), Failure<A>>;
```

The environment shall separate participant-local identity and endpoints from launch-wide metadata
(size, membership, arena and geometry). Local state shall not occupy a shared mutable slot.
Shared metadata shall be initialized once before readers enter and remain immutable.
A host binding may discover the environment; a device entry may receive it as arguments.
`hosts(cx)[rank(cx)]` names the participant's sharing-domain representative, which is a worker
rank; the management leader is a different thing, and it is absent from `hosts` by construction
rather than subtracted from it.

The deployment states which participants are workers and which one leads each of them, in the
launch's own numbering: the same space as `workers` and `leaders`, and the same space a device
backend uses for its launch's warp or block index, so no backend translates between two. The two
lists are parallel: position `i` of `workers` is contract rank `i`, and `leaders[i]` leads it. Their
lengths shall be equal, and a mismatch is refused by name.

`Deployment::new(workers, leaders)` is the only constructor, and it refuses by name rather than
resolve: an empty `workers` (`EmptyDeployment`), a worker named twice (`DuplicateWorker`), a
`leaders` list of a different length (`UnequalLists`), or a rank that is both (`WorkerIsLeader`).
`leaders` of `None` means the workers are unled. There is no empty deployment meaning "every
participant", so a single-participant launch names its one worker. A backend shall still refuse
a rank outside the job (`RankOutsideJob`), because only it knows the job.

The launch description is the launcher's authority, and it carries three things: the participants,
which of them leads which, and how to reach the leaders out of band. The first two are the
deployment and are this contract's input. The third is not the contract's business, and its absence
is not a hole: a leader is a host process, and a host process reaches another host process the way
hosts already do. There is therefore no leader-to-leader route, and a leader's address is not a
participant rank.

That one leader serves many workers, and that the workers it serves share a host, are statements a
launcher makes. The contract does not learn what a host is: it sees only that worker `i` is led by
`leaders[i]`, and a launcher that wants one leader per host gives every worker of a host the same
number. What participants share a domain with each other is discovered, because it is a property of
the runtime and the hardware, and it is discovered within the participant set. What a participant
is supposed to do is decided by the launch, because nothing in the machine says it. The two need not
agree: `hosts(cx)[rank(cx)]` is a sharing-domain fact and is not evidence of leadership, which is
why deriving a leader from it was a machine fact read as a role.

The backend shall establish the participant set before computing any sharing domain, and this holds
per leader: a leader that shares a host with workers - including other leaders' workers - must not
join their domain, and it does not, because it is not a participant. That holds only while the two
steps are in that order. Reordering them admits a non-participant into a worker's domain, and the
symptom is a hang inside the domain computation rather than a wrong number.

The cohort rule shall be pure and allocation-free. Equal returned colours identify one cohort;
its members are ordered by rank. A backend may reject an unsupported grouping at `init`.
No peer operation, plan or collective may precede successful initialization.

Context and environment storage shall outlive their operations. Each physical context object
shall have one owner. Cooperative lanes may own separate replicas; identity, endpoint state and
operation sequence shall remain uniform across replicas. A shared object shall not be aliased
through several mutable references. No implicit process-global context is part of the interface.

`done` shall report the outcome and discharge local obligations; it may return. Observed backend
failures shall produce a participant-visible record for the caller, independent of stderr and
`Drop`. A trap need not write a record: the launcher shall report observed abnormal termination
as failure. Failure detection is not guaranteed after loss of the launcher. Process exit is not
portable, and absence of a failure record is not evidence of completion.

A transport that terminates the participant inside a failing call, before returning a code, cannot
satisfy this clause whatever the caller does: the backend shall configure the transport to return
errors. An operation that still aborts or panics where a record is required is an unsupported
operation, not a reported one.

A failure names its kind: `FailureKind::Backend(BackendFault)` for what the backend observed below
the contract, or `FailureKind::Application(A)` for the application's own, so a record never has to
be decoded from a number.

## 4. Communication

```rust
fn send(cx: &mut Context, to: Rank, channel: Channel, data: &[u8]) -> Result<(), Error>;
fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<Frame>, Error>;
fn flush(cx: &mut Context) -> Result<(), Error>;

enum Error {
    Full, Busy, Closed,
    TooLarge { limit: FrameBytes }, TooSmall { needed: FrameBytes },
    Invalid(Invalid), Failed(Failure),
}
```

`send` success means **accepted**. Before returning, the backend shall copy the bytes into
transport-owned storage or finish using them. No reference to `data` shall escape the call.
Capacity shall be fixed or bounded by the environment. Refusal shall accept no part of a frame;
acceptance shall occur once.

`Message(tag)` shall be reliable and FIFO per `(source, destination, tag)` during a healthy run.
`Lane` uses the geometry and tag established by `reshape`; its loss and ordering guarantees shall
be declared separately. A lossy lane does not permit loss on the message channel. Worker-to-worker
traffic uses `Message` and `Lane` and nothing else.

A worker's route to its leader, and the leader's route to its workers, is `trame::leader`, and it
is a separate route with its own transport:

```rust
pub struct Leader; // the host process's end
impl Leader {
    pub fn open(env: Environment, deployment: Deployment<'_>) -> Result<Leader, Failure>;
    pub fn send(&self, to: Rank, tag: Tag, data: &[u8]) -> Result<(), Error>;
    pub fn recv(&self, out: &mut [u8]) -> Result<Option<Frame>, Error>;
}
pub fn send(cx: &mut Context, tag: Tag, data: &[u8]) -> Result<(), Error>;
pub fn recv(cx: &mut Context, out: &mut [u8]) -> Result<Option<(Tag, FrameBytes)>, Error>;
```

The worker end takes no destination, because the backend knows its leader and nothing derives it
from the numbering. The leader end takes no source, because its sources are workers and they are
ranks, so `Frame` reports them. `leader::recv` returns `(tag, length)`: the worker end has one
source, so there is no source to report and no type is spent on one.

Two verbs over two routes put route arbitration in the caller's hands, which is why there is no
general fairness obligation between them: a worker chooses when to check its leader and when to
check its peers, and neither route can starve the other behind its back. How leader bytes move is
the backend's choice, and it is not the worker-to-worker mechanism.

The leader is a host process reachable by `Leader::open(env)`, mirroring `init` from the other
side. That holds under every backend, a device one included: what a backend declares is the
*route*, not the leader's kind. It may use sockets, files and threads; the contract neither permits
nor forbids that, because the leader never enters the participant surface, so no lowering has to
distinguish it.

A device lowering may realize the control route as memory the participant polls; its `recv` is
then one look at that memory, like every other `recv`.

`recv` shall return one complete frame from any source, with its tag and bytes paired. `None`
means no frame was available.
There is no tagged receive: the caller dispatches each frame by its tag. Broadcast is an
iteration of sends with explicit per-destination outcomes. A backend that matches a probe
atomically may hold the matched frame inside its receive scope when a refusal names it; the next
receive on that scope shall return that frame. Holding a frame a refusal named is not a tagged
stash or a demultiplexer, and a probe cannot be undone, so this retention is required for any
non-consuming refusal.

`TooSmall` shall report the required length without consuming the frame or changing `out`.
This does not prevent a lossy lane being overwritten before the next call. The backend shall
serialize matching and consumption within the receive scope; a competing local receive shall
not invalidate a probed length, and a probe-then-receive pair is not sufficient because a peer
may send between the two, pairing one frame's length with another's bytes. A matched message
shall not be abandoned on refusal. A receive
loop shall provision `MAX_FRAME` or handle `TooSmall` by supplying enough
storage or failing the run; repeating the same undersized receive is not progress.

`flush` success means all previously accepted local sends have released their operation-specific
send resources. It does **not** mean that receivers have consumed or applied them. An
incomplete flush returns `Busy`. MPI eager completion and receiver-released ring storage may
satisfy this obligation at different times without changing its meaning. A backend whose sends
retire inside the call has nothing to wait for and may answer `flush` immediately; that is a
property of its admission, not a stronger guarantee.

`Full` shall be reported whenever an attempt is refused for lack of capacity, including when the
attempt's own failure is the only observation of that capacity. A backend shall not abort, wait,
or drop in that case.

`send`, `recv` and `flush` shall drive local transport progress. No background thread or
asynchronous device progress is assumed. Accepted frames shall not require a caller-owned
handle to remain alive. There is no portable `Inflight`, implicit retry or `Drop`-driven wait.
A failure after acceptance shall fail the run visibly; it shall not become a safe-to-retry refusal.

## 5. Waiting

The backend does not wait. Every `send`, `recv`, `flush` and both leader ends make exactly one
attempt and answer `Full`, `Busy`, `Ok(None)` or an outcome. Waiting is the caller's: it repeats
the attempt, and it decides whether to spin, yield to other work or give up, because only the
caller knows what else it could be doing.

Where a backend's machinery cannot make a genuine single attempt, the site is marked `C3:` in the
source and keeps its current behaviour until it is fixed; it is a defect in that backend, not a
second meaning of the operation.

A device backend shall state its progress assumptions, including residency and convergence.

## 6. Geometry and lifetime

```rust
fn reshape(cx: &mut Context, workers: &[Rank], edges: &[Edge], bytes: u32, tag: Tag)
    -> Result<(), Error>;
fn release(cx: &mut Context) -> Result<(), Error>;
fn partition::slice_of(hosts: &[Rank], domain: &[Rank], rank: Rank, total: usize)
    -> Result<ByteRange, Invalid>;
fn share(cx: &mut Context, mine: &[u8], total: usize) -> Result<Shared, Error>;
fn bytes(segment: &Shared) -> &[u8];
fn unshare(cx: &mut Context, segment: Shared) -> Result<(), (Shared, Error)>;
```

Workers shall be ascending and unique. Edges shall be ordered by `(source, destination)`, without
duplicates; both endpoints shall be workers. `affected` is the number of destination elements
reachable from that source; `bytes` is the lane frame capacity. Validation shall not require a
hash map. Invalid or unsupported geometry shall be refused at `reshape`, before any lane send.

A backend may use one launch-wide geometry. It shall validate every declared pair against that
geometry rather than resize or truncate silently. `reshape` shall not replace live resources.

`release` discharges the caller's lane obligations. Its declared guarantee is either local
release or cohort-wide quiescence. Local return is not evidence that peers stopped accessing
storage. A caller requiring global reclamation shall require the stronger capability or provide
an explicit protocol proving every outstanding access has ended.

The layout rule is pure, allocation-free and published as `partition::slice_of(hosts, domain,
rank, total)`, whose domain is a parameter and whose alignment is the selected backend's. It is
the one definition of where the cuts are: `share` applies it to the caller's own domain, and a
caller that must size another domain's contributions, as a launcher does for its workers, calls it
with that domain stated.

`share` shall expose one immutable copy per sharing domain. `share` takes no domain because
publication is a capability: only a member can expose a copy as that domain's copy, so a domain
argument would promise what no backend could honour. Publication shall validate coverage and
total size. A backend may require collective publication or use an allocation installed before
participant entry. Returned bytes shall not become visible before publication is complete for
that reader.

`unshare` shall consume the mapping handle on success and return it on refusal.
Physical reclamation requires the backend's stated completion boundary: collective
retirement or an enclosing lifetime whose readers have all finished. Neither dropping a handle,
quiet statistics nor a local transport flush proves that boundary.

## 7. Clocks

```rust
fn clock::reading() -> Reading;
fn clock::unix_nanos() -> Result<i64, Unrepresentable>;
```

A reading is a property of the machine rather than of the participant, so the call takes no
context: a host has one process clock and a device one counter per power cycle, and parameterising
by participant would give one fact two spellings. The identity is what says which readings may be
compared, and a backend whose identity is recomputed per call has no comparable readings at all.

Readings shall be monotonic within one clock identity. Only readings with that identity may be
subtracted: `Reading::since` refuses any other pair with `ClockMismatch`. `Span` is integer nanoseconds, not a standard-library duration; a backend shall state
resolution, conversion from native ticks and overflow behavior. Conversion shall not invent
cross-context synchronization. Realtime is signed nanoseconds since the Unix epoch and may move
backwards. An unavailable realtime clock shall cause a compile error, not return zero or `None`.

## 8. Shared-state primitives

Exactly three families are required by workloads that use shared mutable state. A backend shall
declare the scopes and operations it supports; transport reliability does not imply any of them.

| Family | Required semantics |
|---|---|
| A: turn | One mutable owner; nonpreemptive release. Among waiters, higher priority first and FIFO among equals. Default priority is 1. Acquisition supports polling, declared waiting and explicit cancellation. |
| B: publication / transfer | Publication exposes whole committed versions from a private writer draft. Reliable transfer moves ownership once. Both use bounded storage; refusal preserves the writer's draft or returns the transferred payload. Capacity, contention and closure remain distinct. |
| C: atomics | Conventional atomic operations with stated ordering and visibility scope. No fairness or cross-scope visibility is implied. |

A FIFO ticket is a valid Family A implementation when only equal priorities are supported;
nondefault priorities shall then be refused at compilation. The family does not require a waiter
table, parking thread or host mutex. Cancellation and wait exhaustion shall be distinct outcomes.
Family B shall state which side may wait; writer publication and transfer admission shall not.

## 9. Execution and lowering

```rust
#[trame::concurrent]
fn worker(&self, role: Role, cx: &C) -> Result<(), E>
#[trame::parallel]
#[trame::ordered(key = f.target: Target)]
fn integrate(&self, f: Frame, slot: &mut Slot, cx: &mut PassCx) -> Result<(), E>

trame::invoke!(self.worker, &cx, &[Role::Intake, Role::Delivery])?;
trame::invoke!(self.integrate, &mut pass, &frames, trame::Keyed::new(&mut slots[..]))?;
```

A `#[parallel]` function is one unit of data-parallel work; a `#[concurrent]` one is one unit of
concurrent work. The first parameter after `&self` is the item; `invoke!` runs the function once
per item of a `&[I]`, `I: Copy`, joins every call, and returns the first `Err` in list order. An
`Err` does not stop the other items. The attribute rewrites the function into a driver whose first
parameter is a `trame::Invocation` only `invoke!` constructs; the body exists only inside it.

A `#[parallel]` context is `&mut C`: each call owns it (host: calls run in list order on one
thread; nv: every lane has its own). A `#[concurrent]` context is `&C`, shared, and mutated only
through §8's families. No other parameter may be `&mut` except an `#[ordered]` function's one
keyed slot. `#[ordered(key = place: K)]` names a place in the item and its type; `invoke!` then
takes a `trame::Keyed<K, T>` and hands each call the slot its key names. Equal keys run on one
lane in issue order; different keys are unordered. A key naming no slot is
`Invoked::OutOfRange`, never a skip. The key's type is checked against `K` and `K` against the
`Keyed` the caller passes. Anything else is refused by name at expansion.

Host lowering: `#[parallel]` is the loop over the list; `#[concurrent]` is `std::thread::scope`
with one thread per item. nv lowering: unordered `#[parallel]` strides the list over the warp's
32 lanes (`warp::Split`); ordered `#[parallel]` has every lane walk the list and run the items
whose `key % 32` is its lane; both are bracketed by `warp::sync()`. Each lane answers for its own
items. nv has no `#[concurrent]`: a participant is one warp (`here_id` is the launch index over 32
lanes), so there is no second warp to give an item, and the attribute is refused.

A context that cannot be copied is shared between a participant's concurrent units through a
Family A exclusive, which lends it for one call. A unit that holds it across a wait starves its
sibling; nothing can check that, so it is an author obligation, and the retry loop sits outside
the guard.

A device lowering compiles the call graph reachable from an entry, not the whole crate. Host-only
code outside that graph is not lowered and need not be excluded from the crate. A body inside that
graph shall not reach a device intrinsic through a helper the lowering cannot analyse: annotate the
helper for the device, or read the per-lane value at the entry and pass it as an argument.

For warp-cooperative transport, every lane shall execute `send`, `recv` and `flush` in convergence,
with equal buffer addresses, lengths, channels and destinations where applicable. Endpoint
sequences shall remain equal. Buffers shall not overlap transport storage. Different lane-local
frames shall be assembled into one uniform frame before sending, or the work shall remain serial.

A cooperative receive shall use disjoint lane writes under the logical exclusive borrow of §2.
Native address-and-length operations may implement it; ownership shall not be weakened.

The portable worker uses these attributes and the selected backend's primitive families.
`cpu::exec`, `cpu::sync` and `cpu::clock` are host implementation details, not portable imports.
No capability requires a run-time registry, scheduler object or trait hierarchy.

## 10. Conformance

Conformance requires checks independent of NERVE, using synthetic payloads. Required evidence:
deployment refusal; environment and cohort agreement; a `MAX_FRAME` frame on every route; frame
ownership, refusal and ordering; one attempt per call; geometry refusal; sharing lifetime; and
clock context separation.

For the resident-device backend, evidence shall additionally cover distinct per-participant
identity and endpoints, convergent transport calls, fixed storage, a full lane, a short receive
buffer and an unscheduled peer. Host-model initialization shall match device initialization;
a passing host model alone is insufficient. Numerical equivalence remains a separate NERVE check.

On device (RTX 2080 Ti, sm_75): a partitioned dispatch region (the predecessor of `#[parallel]`) ran 96 indices over 32 lanes with
each index executed exactly once; a 64-byte round trip passed; a short buffer was told it needed
64 bytes with the frame still present afterwards; a full lane refused the next send rather than
overwriting; and a bounded wait against a peer that was never launched terminated after exactly
its stated attempts and reported, rather than hanging.

`request.md` items map to sections: 1–2 → §3; 3 → §2; 4 → §4; 5 → §5/§8;
6 → §6; 7 → §9; 8 → §6; 9 → §7; 10–12 → §9; 13 → §1/§9.
Current gaps include the global entry API, allocating transport signatures, host-only worker
imports, send handles, missing capability refusals and differing declaration metadata.
NV additionally has shared mutable rank and endpoint state (`launch::WORLD`, `peers::cuda::LINKS`)
and concurrent cohort initialization. These violate §3; the host adapter does not validate them.
The MPI family reports capacity refusal on its message route (`MPI_ERRORS_RETURN`, raw `MPI_Bsend`
mapped to `Full`) but its other calls go through wrappers that panic on a non-success code, so
their failures are not yet records. That is a §3 gap, not a §4 one.
