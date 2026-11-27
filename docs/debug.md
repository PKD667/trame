# The debug dump

An opt-in, compile-time record of every trame primitive: what a run did, as metadata, in memory
while it runs and in a binary file when it ends. It exists to debug a program's *behaviour* — which
step ran when, what a worker sent and received, where a leader waited — without changing the
program. It records no message payloads and no step results: metadata only.

The switch is the `--cfg debug_dump` rustc cfg, off by default. It is not a cargo feature: the
backends, not the features, decide what a build is, and the dump is orthogonal to them. Every cfg
that has to do with the dump lives in `src/debug.rs`; the rest of the tree records with one-line
calls that compile away when the cfg is off.

## Build

```
RUSTFLAGS='--cfg debug_dump --cfg trame_checked' cargo build --no-default-features --features <backend>
```

`trame_checked` is not required by the dump, but the two belong together in practice: the dump
answers questions about a run the checked build replays. A debug_dump build has not passed the
driver's step checks (CONTRACT.md M4): recording adds effects a step's body would not otherwise
have — the cursor `fetch_add` and a 32-byte store — so a build with the dump on is a debugging
build, not a certified one.

## Environment

| variable            | default  | meaning                                                                |
|--------------------|----------|-----------------------------------------------------------------------|
| `TRAME_DUMP_DIR`    | unset    | where `rank<rank>.dump` files are written. Unset: the process records nothing. |
| `TRAME_DUMP_BYTES`  | 268435456 | the arena's size in bytes. Unreadable or absurd: the default, clamped to [4096, isize::MAX). |

The launchers pass their run's `--out` directory as `TRAME_DUMP_DIR` when the program sets none
(each backend's `launch`), so a dump lands beside the run's outputs
when the program was built with the cfg.

## The file

One file per process: `rank<rank>.dump` in `TRAME_DUMP_DIR`, `rank` being the process's rank in the launch
rank on a host of several processes and 0 where a host is one process. A 52-byte header, then `count` records of 32 bytes, all
little-endian:

| offset | size | field                  |
|--------|------|------------------------|
| 0      | 8    | magic `TRAMEDMP`       |
| 8      | 4    | format version u32 = 1 |
| 12     | 2    | host u16               |
| 14     | 2    | rank u16               |
| 16     | 8    | launch id u64           |
| 24     | 8    | arena size u64          |
| 32     | 4    | record size u32 = 32    |
| 36     | 8    | count u64               |
| 44     | 8    | t0 wall clock u64       |

The launch id is the process's `t0` as nanoseconds since the epoch (`SystemTime`), so it also names
the run: files of one directory with the same t0 are one launch. One record:

| field  | type | meaning                                      |
|--------|------|----------------------------------------------|
| t      | u64  | nanoseconds since this process's launch      |
| a      | u64  | the op's first word (below)                  |
| b      | u64  | the op's second word (below)                  |
| rank   | u32  | this process's rank                          |
| host   | u16  | this process's host                          |
| agent  | u8   | the thread's agent id                        |
| op     | u8   | the op code (below)                          |

## The ops

"Route id" has two numberings, and the table says which one. A worker-side send, receive or room
names the route as `Backend::worker_route` numbers it: the index among *this worker's outgoing
routes* in plan order, the same number `Status::room` waits on. A cross-host lane names the route
by its `RouteId`: the route's index *in the topology*, the same on both hosts. A "type tag" is the
F4 tag of the message type in hand — `io::tag::<M>()` of the payload type `M`, also on a route to
the leader, where the wire type is `Wire<M>`.

| code | op          | a                             | b                                    |
|------|-------------|-------------------------------|--------------------------------------|
| 1    | STEP_START  | step index                    | 0                                    |
| 2    | STEP_END    | step index                    | status: 0 progress, 1 wait, 2 done, 3 error, 4 fresh |
| 3    | SEND        | type tag                      | outgoing route id                    |
| 4    | SEND_FULL   | type tag                      | outgoing route id                    |
| 5    | RECV_HIT    | type tag                      | first incoming route id of the type  |
| 6    | RECV_MISS   | type tag                      | 0                                    |
| 7    | WAIT        | what: 0 wait(∅), 1 room, 2 frame | type tag for a room, else 0      |
| 8    | WAKE        | epoch before the idle         | epoch after the wake                 |
| 9    | EDIT        | target: 1 worker, 2 pool      | worker index, 0 for the pool         |
| 10   | STOP        | 0                             | 0                                    |
| 11   | JOIN        | workers of the run            | 0                                    |
| 12   | PEER_SEND   | host sent to                  | peer message tag                     |
| 13   | PEER_DRAIN  | messages moved into pending   | arrivals so far                      |
| 14   | PEER_WAIT   | 0                             | 0                                    |
| 15   | SHARE       | bytes shared                  | 0                                    |
| 16   | RELAY_IN    | lane's RouteId                | frame bytes                           |
| 17   | RELAY_OUT   | lane's RouteId                | frame bytes                           |
| 18   | EPOCH_BUMP  | epoch word's byte offset      | epoch value after the bump            |
| 19   | DROP        | records dropped               | 0                                    |
| 20   | MARK        | user marker                   | 0                                    |

Where a user marker comes from: `trame::mark(n)`, a name that exists only in a `--cfg debug_dump`
build (so a build without the cfg has the surface it had before). A step may call it: one atomic
and a 32-byte store.

Records a backend has no call site for never appear in its files: a single-process host records no WAKE, EPOCH_BUMP,
PEER_* or RELAY_*; a host of several processes records all of them. `b` of RECV_HIT names the *first* incoming route of the
type in plan order: a fan-in of one type from several senders is named by that first route, because
the receive's surface does not know the sender. Frames that arrive on a lane but are never taken,
and frames a pending lossy lane drops at the end of a run, record nothing.

## Recording

Each process boots one arena of `TRAME_DUMP_BYTES` bytes at its launch (64-aligned, never freed).
The arena is allocated with `std::alloc::System` directly, not through the program's `trame::Alloc`,
so it lands in no domain and counts towards no worker's memory share (CONTRACT.md M8): the dump
measures the run without being part of what the run's refusals budget for. Recording is one
`fetch_add` of 32 on the arena's cursor and a 32-byte store: no allocation, no lock, no syscall
(the timestamp is a vDSO `CLOCK_MONOTONIC` read on Linux), never blocking. The last 32 bytes of the
arena are reserved: when the arena fills, records stop being written and are only counted, and the
dump ends with one DROP record — the reserved slot — saying how many were counted. `t` is
nanoseconds from the process's launch, so timestamps are monotonic per process.

Agent ids are one byte per thread of the process, dense: 0 is the main thread, and every thread a
backend spawns takes the next id where it starts. The 256th and later threads share 255.

## Triggers

A dump writes the file from the arena, and overwrites what an earlier dump wrote:

- at `Run::join` and the checked `join`, after the JOIN record;
- when a worker's panic aborts the run, at the leader's report of it, on both backends; a worker
  rank of a multi-process host dumps at its own exit, which every worker rank reaches;
- on `SIGUSR1`: the handler only sets an `AtomicBool` (async-signal-safe: no allocation, no syscall),
  and an agent loop that passes its cheap poll — `debug::check_requested()` — writes the file. The
  reference scheduler's agent loop has the poll, and so do a single-process leader's wait and a multi-process
  leader's wait.

## Honesty

- `t` is nanoseconds since this process's launch. Across processes of one host the header's `t0`
  aligns them on the kernel's wall clock; across hosts the alignment is as good as the hosts' clock
  sync. `timeline.py` prints times from the earliest aligned point of the directory.
- A dump caught by SIGUSR1 may include a record another thread is writing mid-write. The dumps at
  join and at a rank's exit happen after the threads have stopped recording.
- The metadata says what was sent and when, never what: no payloads, no state.
- A run that ends without `join` and without an abort writes no dump (only a leader's run knows it
  ended); the SIGUSR1 poll covers the wait loops listed above, and a thread parked in a wait may
  serve the signal later than the thread that runs.
- Recording costs one atomic and a store on the hot paths it covers; the dump does not measure
  latency-free programs, it measures what the program did anyway.

## Readers

`check/debug/` holds the suite, plain `python3`, no packages:

- `read.py` — the format as a module: the op table and `read_dump` / `read_dir`.
- `timeline.py DIR [--rank N ...] [--agent N ...] [--op NAME|N ...] [--since NS] [--until NS]` —
  one merged chronological timeline, times in nanoseconds from the earliest aligned point.
- `stats.py DIR [--top N]` — per-op and per-agent counts, the longest WAIT intervals, SEND_FULL
  counts, PEER_* gaps, per-worker step throughput. A record's worker is its (rank, agent) pair: on
  a single-process host has agent 0 as its leader and agent k+1 as worker k; a multi-process host has a worker per rank.

## A worked example

`examples/ring.rs` on seq, four workers passing a token three rounds and reporting to the leader:

```
$ RUSTFLAGS='--cfg debug_dump --cfg trame_checked' cargo build --no-default-features --features <backend> --example ring
$ rm -rf /tmp/ring && TRAME_DUMP_DIR=/tmp/ring ./target/debug/examples/ring
worker 0 total 9
worker 1 total 8
worker 2 total 7
worker 3 total 6
outcomes: [Returned, Returned, Returned, Returned]
$ python3 check/debug/timeline.py /tmp/ring
# timeline of /tmp/ring: 61 record(s), t in ns from the earliest aligned point
           t  rank host  ag  op                          a                  b
      495177     0    0   2  STEP_START                  0                  0
      575377     0    0   1  STEP_START                  0                  0
      650046     0    0   4  STEP_START                  0                  0
      745255     0    0   3  STEP_START                  0                  0
      783957     0    0   2  SEND       9600620431234051292                  0
      836024     0    0   1  SEND       9600620431234051292                  0
      885176     0    0   4  SEND       9600620431234051292                  0
      923889     0    0   3  SEND       9600620431234051292                  0
      953234     0    0   2  RECV_HIT   9600620431234051292                  0
      958193     0    0   2  STEP_END                    0                  0
      ...
$ python3 check/debug/stats.py /tmp/ring
file rank0.dump: rank 0 host 0 61 record(s), arena 268435456 bytes, t0 1795772764494574695

per op:
  JOIN                1
  RECV_HIT           12
  SEND               16
  STEP_END           16
  STEP_START         16

per agent (rank, host, agent): records
  rank 0 host 0 agent 0:          1
  rank 0 host 0 agent 1:         15
  rank 0 host 0 agent 2:         15
  rank 0 host 0 agent 3:         15
  rank 0 host 0 agent 4:         15

longest WAIT intervals (to the same agent's next record): 0 in total

SEND_FULL counts: 0 in total

longest PEER_* gaps (between consecutive peer events of a rank): 0 in total

per-worker step throughput (steps of one (rank, agent)):
  rank 0 agent 1: 4 steps in 1.119 ms (3576 steps/s, 0.280 ms/step)
  rank 0 agent 2: 4 steps in 1.186 ms (3373 steps/s, 0.296 ms/step)
  rank 0 agent 3: 4 steps in 0.981 ms (4078 steps/s, 0.245 ms/step)
  rank 0 agent 4: 4 steps in 1.059 ms (3778 steps/s, 0.265 ms/step)
```

The token's tag is `9600620431234051292` (`io::tag::<Token>()`) on route 0, the report's is
`11453855225316045` on route 1, the leader's route; the last hops end with `STEP_END b=2` (done) and
`JOIN a=4` comes from agent 0, the leader. No WAIT and no RECV_MISS: seq's baton hands each worker
the token after the sender's step, so no receive ever misses — the dump says so, and it is true.
