# trame contract

## General

| Term | Definition |
|---|---|
| Unit | Logical owner of state that advances independently. |
| Domain | Declared set of units sharing storage. |
| Deployment | One host's leader, workers and shared storage. Peer routes may cross hosts of the same launch. |
| Launch | Assigns workers, locations, numberings and links. |
| Interface | Functions over handles, four per-step `Io` methods, macros and attributes. One implementation selected at build time. |

**Implementation.** Preserve ownership, visibility, ordering and outcomes. Copying, polling,
serialization and parallel execution are permitted lowerings that preserve operation meanings.
`#[parallel]` and `concurrent!` permit sequential execution. FIFO, handoffs and execution declarations
are library guarantees. Impossible launches/geometries refuse before use; limits are explicit.
Compiler and hardware limitations restrict implementation support. The contract remains unchanged.

**Compatibility.** Documented functions, macros, attributes, fields and bounds are identical across
selections. Private layouts, incidental traits and unstable compiler marker traits are excluded.

## Entry and finalization

| Interface | Result |
|---|---|
| `Launch(u32)`, `Tag(u16)` | `new`, `get`; start no work. Every `u16` tag is valid. |
| `Deployment::new(hosts: &[&[Launch]], here: u16, leader: Launch)` | `Result<Deployment, Invalid>` |
| `init(Environment, Deployment)` | `Result<Context, Failure>` |
| `rank(&Context)`, `size(&Context)` | Local rank/count, `u32`. |
| `done<A>(&mut Context, Result<(), Failure<A>>)` | `Result<(), Failure<A>>` |

**Assignment.** Every launch process receives the same table. `hosts[h][r]` identifies worker `r`
on host `h`; row `here` assigns dense ranks `[0, size)`. Rank is distinct from `Launch`.
One external leader per deployment, identified by `Launch`. Every worker answers to it.
The implementation assigns the leader's execution resource. Its launch identity is absent from the worker table.

**Invalid entry.** Empty tables/rows, unnumberable tables, invalid `here`, duplicate identities,
listed leader; additionally at `init`, foreign participants, inconsistent entry data or insufficient
storage. No silent deployment narrowing. `Environment` is opaque with `Default`.

**Ownership.** `Context` owns entered resources. `Context`, `Io`: `Send`, not `Sync`.
`Shared`, `Published`, `Leader`: neither; views/operations retain owner lifetimes.
Borrowed byte slices may be shared where Rust permits.

**Finalization.** Normal return preserves the supplied outcome and ends handle use. Pending frames
may be lost; delivery and receipt are not required. Unrecoverable failure is abnormal termination,
never success. Entered participants finalize explicitly; drop is neither successful finalization nor
collective participation. Traps/killed processes guarantee no destructors.

## Communication

**Format.** Frames carry caller-supplied bytes. Lengths, offsets, capacities: bytes, Rust
`usize`. `Frame::source`, `tag`, `len` describe a whole received frame; partial receives forbidden.
`Channel`: `Message(Tag)` or `Lane`.

| Interface | Result |
|---|---|
| `send(&mut Context, Addr, Channel, &[u8])`, `flush(&mut Context)` | `Result<(), Error>` |
| `recv(&mut Context, &mut [u8])` | `Result<Option<Frame>, Error>` |
| `Leader::open(Environment, Deployment)` | `Result<Leader, Failure>` |
| `Leader::send(&self, to: u32, tag: Tag, data: &[u8])` | `Result<(), Error>` |
| `Leader::recv(&self, out: &mut [u8])` | `Result<Option<Frame>, Error>` |
| `Leader::done<A>(&mut self, Result<(), Failure<A>>)` | `Result<(), Failure<A>>` |
| `leader::send(&mut Context, Tag, &[u8])`, `leader::recv(&mut Context, &mut [u8])` | Worker-to/from-leader operations |

| Outcome | Condition/effect |
|---|---|
| `Full`, `Busy`, `Closed` | No capacity, contention, closed route/endpoint. |
| `TooLarge { limit: usize }` | Exceeds byte limit. |
| `TooSmall { needed: usize }` | Buffer insufficient; frame retained. |
| `Invalid(Invalid)`, `Failed(Failure)` | Invalid request, observed backend failure. |
| `Ok(None)` | No eligible frame received. |

**Operation.** One attempt; refusal accepts/consumes nothing and hides no wait. Caller may retry,
do other work or stop. Backend failures use `Failed`. Send privately copies bytes before return;
caller buffer reusable immediately. Flush certifies only accepted sends' send-side storage release. Publication precedes receipt and exposes every payload byte.

**Ordering.** Message: reliable FIFO per sender/receiver pair across tags while live. Lane: FIFO
per pair; explicitly lossy lanes may skip, never reorder. No order between channels. Closing routes
or participants need not deliver pending frames.

**Progress.** Live routes cannot permanently refuse admissible traffic by failing to service work.
Assumes live launch, execution opportunities and required receives. No latency bound or delivery
to a nonreceiving application.

**Leader routes.** Same attempt/frame/finalization rules; FIFO per worker in each direction.
Worker receives source `None`; leader receives `Some(Local(r))`. The leader endpoint uses `Leader::open`, `Leader::send`, `Leader::recv` and `Leader::done`. Workers use the free functions `leader::send` and `leader::recv`.

**Leader exchange.** `Leader::exchange(outgoing)` gives `outgoing[h]` to host `h`'s leader and
returns every host's piece for this one, indexed by host. It is collective over the launch's
leaders: each enters once per exchange, in the same order, with one buffer per host, else
`Invalid(RankOutsideJob)`. Leaders reach each other only here; no worker carries leader traffic.
MPI runs it among a leaders-only communicator; `none` and `nv` exchange within one host.

### Addresses and lanes

| Address | Validity |
|---|---|
| `Local(u32)` | Rank below `size`. |
| `Remote { host: u16, rank: u32 }` | Worker on another launch host. |
| Invalid address | Nonexistent worker or remote naming this host: `Invalid(RankOutsideJob)`. |

**Use.** Numberings remain fixed. Addresses occur in `send`, `Io::send`, `Edge`, `Frame::source`;
host-local rank operations, `reshape` workers and `Leader::send` use `u32`.

**Links.** Preserve channel semantics, one-attempt outcomes and FIFO per directed pair. Never lose
frames, including with local `LOSSY` lanes. Unsupported remote addresses refuse at the call, by
name. Acceptance certifies send-side copying only. `reshape`, `release`, `barrier` remain host-local.

**Configuration.** `reshape(&mut Context, workers: &[u32], edges: &[Edge], frame: usize, Tag)`
and `release(&mut Context)` return `Result<(), Error>`.

| Requirement | Rule |
|---|---|
| Geometry | Workers ascending/unique; edges ascending by `(source, destination)`. At least one local endpoint per edge; every local endpoint listed. |
| Capacity | `Edge::new(source, destination, affected)`: nonzero destination-element count for planning. `frame`: byte limit. Geometry is backend-owned; unsupported geometry refuses before sends. |
| Traffic | Declared tag. Unconfigured: `Invalid(LaneNotConfigured)`; undeclared pair: `Invalid(NoLane)`; oversized: `TooLarge { limit: frame }`. |
| Release | Ends local lane use; does not certify peer consumption. |

## Shared storage

| Interface | Result |
|---|---|
| `leader::publish(&Leader, NonZeroU64, &[u8])` | `Result<Published, Error>` |
| `leader::handle(&Published)` | `Handle` |
| `leader::retire(&Leader, Published)` | `Result<(), (Published, Error)>` |
| `unsafe attach(&mut Context, Handle)` | `Result<Shared, Error>` |
| `bytes(&Shared)` | Borrowed `&[u8]` |
| `detach(&mut Context, Shared)` | `Result<(), (Shared, Error)>` |

**Publication.** Leader owns segment and is sole writer. Reserve all storage, write all bytes,
publish revision; no later writes. Revisions may coexist; successor needs both lengths. Refusal
leaves predecessor/readers untouched. Capacity failure refuses with cause before copying.
Republishing a live revision is the leader's error and may refuse.

**Handle.** Revision, length, private token; fixed-size encoding via `BYTES`, `to_bytes`,
`from_bytes`. Decode accepts all bytes except revision zero; attach validates segment.
Handles travel in frames; segment bytes never do. Segment length is not bounded by `MAX_FRAME`.

**Attachment.** Map read-only; acquire revision before other header fields/payload.
Revision/length/format mismatch: `Invalid(NoSegment)`. Open/map failure:
`Failed(BackendFault::Os(errno))`, including ENOENT after retirement/noncolocation.
Unsafe caller prevents retirement until detach/drop; retirement may free live-view memory.
`Shared` owns mapping; `bytes` borrows it.

**Cleanup.** Detach/retire consumes handle on success; refusal returns same live handle and precise
OS error. Failure sticky: later detach/retire calls return same errno without syscall. Owner alone retires;
retirement on reader asserts. Drop cleanup failure reports and aborts, never panics/disappears.
Unhandled refusal dropped: report original error, abort without retry. Leader retires after every
attached worker reports detach by ordinary frame. Crash-left segments require operator cleanup.

**Domain.** Colocation is a launch precondition: leader/workers share one domain. Entry/attach
refuses violations by name; no discovery/repair. Mutable sharing uses exclusive access, handoff or
atomics, never aliased `&mut`. Publication/acquisition establishes visibility regardless of
instructions; no coherence beyond domain. Rust-value primitives require one worker's valid address
domain. Pointer validity is required independently of `Send` and `NoUninit`;
borrowed values remain accessible to logical owners.

## Synchronization and arithmetic

**Exclusive access.** `Exclusive::new(T)` owns state; `Exclusive<T>: Sync` when `T: Send`.
`sync::with<T, R>(&Exclusive<T>, impl FnOnce(&mut T) -> R) -> Result<R, Locked>` replaces
`Exclusive::with`. One attempt; success calls body once with sole mutable borrow. Release publishes
to next successful acquisition. Contention: `Busy`; escaping unwind: later `Abandoned`; trap: launch
ends. `R` need not be padding-free/copyable and is not duplicated across lanes.

**Handoff.** `sync::handoff` functions replace current constructor/endpoint methods.

| Function | Result/effect |
|---|---|
| `new<T, const D: usize>(impl FnMut() -> T)` | `Handoff<T, D>`; positive item count `D`, `D` spares, ready capacity `D`. |
| `split(&mut Handoff<T, D>)` | One borrowed `Sender<'_, T, D>` and `Receiver<'_, T, D>`. |
| `spare(&mut Sender)`, `recv(&mut Receiver)` | `Result<T, Idle>`; spare or oldest ready value. `Idle`: `Empty`, `Busy`, `Closed`. |
| `send(&mut Sender, T)`, `give(&mut Receiver, T)` | `Result<(), Unsent<T>>`; publish ready/return spare. `Unsent<T> { why, value }`: same refused value, `Full`, `Busy`, `Closed`. |
| `sending(&Receiver)` | `bool`: sender exists. |

One attempt; publication covers ownership-accessible data. Sender closure preserves queued values.
Split again after both endpoints disappear. Value occupies exactly one caller/spare/ready location;
moves never duplicate, reclamation drops retained values once. Endpoint drop fabricates no
acceptance. Cross-thread endpoints require `T: Send`; no `Copy`/`NoUninit` bound.

**Atomics.** `sync::atomic`: `AtomicBool`, `AtomicU32`, `AtomicU64`, `Ordering`; Boolean, `u32`,
`u64` widths. Operations: `new`, `load`, `store`, `swap`, `compare_exchange`, `Default`, integer
`fetch_add`. Rust ordering within domain; serialization preserves acquire/release visibility.

**Arithmetic.** `trame::optim::exp(f32) -> f32`: portable range-reduced arithmetic on `[-30, 0]`,
relative error below `2e-6` against `f64::exp`. Every export has portable definition; backends may
shadow with better target implementation. Arithmetic permits LLVM vectorization.

## Coordination

Matching participants enter matching boundaries in matching order. `barrier(&mut Context) -> ()`.

| Boundary | Participants/effect |
|---|---|
| Entry | Every declared worker: `init`; each deployment leader: `Leader::open`. |
| `reshape`, `release`, `barrier` | All entered workers, no leaders. `reshape` workers select active geometry; collective membership remains all entered workers. Declarations agree. |
| Barrier | No frame consumption, send finalization or delivery certificate. Finish prior exchanges first. |
| `publish`, `retire` | Leader alone; retirement follows attached-worker detach reports. |
| `attach`, `detach` | One worker, no coordination; handles arrive in application frames. |
| Finalization | Every entered worker: `done`; leader: `Leader::done`. Joint resources may coordinate completion, never require delivery. |

**Scheduling.** Schedule all admitted participants needed by a boundary; simultaneous residency
not required. No boundaries inside bounded steps/items; bodies finish without waiting for another
body to be scheduled. No intrinsic timeout; launcher terminates stalled/failed launches. Impossible
resources refuse at entry/configuration, never wait for participants that cannot run.

## Execution declarations

### Processes

**Declaration.** `#[trame::process]` preserves the struct and generates an inherent `__declared` marker method. `concurrent!` calls that method, then Rust checks the inherent `step` call and infers its error type.

**Invocation.** `trame::concurrent!(a, b, ...)?`: bind each declared object once in source order before stepping; move unless explicitly borrowed mutably. Objects require `Send`, allow scoped borrows and use a compatible `E: Send`; retained state cannot overlap itself. `Progress`: advance; `Idle`: no advance, permit others;
`Done`: permanent completion. Live unfinished processes get repeated opportunities; interleaving,
threads, polling rate and completion time unspecified. Repeated bounded sequential stepping valid;
finishing a process before admitting its communicating peer invalid. No lane naming/warp votes.

**Failure.** Error stops new steps; admitted steps may finish and are joined. First recorded failing
process by source order wins; error sets may vary. Host panic: join, resume; device trap: launcher
reports. Owners/borrows survive admitted work.

**Communication.** `trame::concurrent!(cx; recv(TAG_SYS) => control, recv(..) => intake, delivery)?`
supplies `step(&mut self, &mut Io<'_>)`. One-step borrow; `Io` alone has endpoint methods, all
`&mut self`: `send(Addr, Channel, &[u8])`, `lead(Tag, &[u8])`, `recv(&mut [u8])`, `flush()`.
Worker routes/results apply; `lead` reaches assigned leader. `recv(A, B)` assigns tags, `recv(..)`
all tags; first source-order match owns peer/lane/leader frames. Unassigned frames stay at backend;
unassigned receiver: `Invalid(NotReceiving)`. Per-channel FIFO across tags between one sending and
one receiving process; no order across receivers. Frames may wait behind same-sender frames owned
by another process; no dispatch loss/reordering.

### Items

**Declaration.** `#[trame::parallel]`: `trame::invoke!(function, &context, &items)?`.
`#[trame::ordered(key = item.target: Target)]`: key indexes exactly one slot of final argument
`trame::Keyed::new(&mut states[..])`. Only that slot mutably borrowed; other mutation uses shared
primitives. Context `&C`, `C: Sync`, optional `&self`: shared, read-only inputs.

**Operands.** By-value items from `&[I]`, `I: Copy + Send`; no `Sync` bound.
Keys: `Copy + Into<usize>`, types distinguish keyed views. Slots: `T: Send`; errors: `E: Send`.
Device lowerings preserve these public Rust bounds.

**Operation.** Visit each item once; join before return. Item errors do not cancel later items.
Out-of-range: `Invoked::OutOfRange { key, len }`, no body call; body failure: `Invoked::Failed(E)`.
First list-order failure wins. Empty input succeeds without calls. No-result body: `E = Infallible`;
keyed range failures remain possible. Repeated keys legal, list-ordered, nonoverlapping; distinct
keys may run in parallel without further order. Sequential list-order execution valid.

**Restrictions.** No per-lane context copies, duplicate effects, error replay, precision changes or
reordered same-target updates. Rust ownership remains; declarations do not make collectives safe.
One result owner receives retained error by move; no `Copy`, `Clone`, `NoUninit` or device-serialization
bound on `E`. Other errors drop once before normal return, order unspecified. Physical broadcasting
cannot strengthen bounds. Host panic joins admitted work before unwinding, need not admit remaining
items. Device trap: launcher reports, no completion/destructor guarantee.

## Failure, clocks and evidence

| Item | Contract |
|---|---|
| `Failure<A = Infallible>` | Fields: `participant: Participant`, `operation: &'static str`, `kind: FailureKind<A>`. |
| `FailureKind<A>` | `Backend(BackendFault)` or `Application(A)`; application/transport faults distinct. |
| `Participant` | `Worker(u32)`, `Leader(Launch)`, `Entering(Option<Launch>)`; entry never invents rank. |
| Failure effects | Observed backend failures identify participant/operation. Accepted effects remain; no rollback/replay. Failed receive: `Error::Failed`, never data. Trapped/lost participants need not record failure; launcher reports abnormal termination. |
| `clock::reading() -> Reading` | Integer `Span`, `u64` nanoseconds from origin, tagged `ClockId`. `Reading::since` rejects differing identities with `ClockMismatch`. No global synchronization; macros do not sample/substitute application time. |
| `ID: Backend` | Selected implementation for diagnostics/records; existing `wire_id` values append-only. |
| `LOSSY: bool` | Lane overwrites only. |
| `MAX_FRAME: usize` | All routes, including leaders. Provision 65,544 through `u32::MAX` bytes or refuse entry; library profile. |
| Build tools | `trame/backends/<b>/cargo`, package `<b>-cargo`, executable `cargo-<b>`. No-tool backends need no placeholder; outside execution interface. |
| Conformance | Observable effects determine conformance. Record specification version, source snapshot, compiler, configuration, hardware. |
| Evidence | Repair failures or leave uncertified. Contract changes require explicit migration/new evidence. Host models, compiler probes and hardware runs remain separate. |
