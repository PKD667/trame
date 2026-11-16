# Scaling benchmarks

Five trame programs and three baselines. The trame programs are examples `bench_round`, `bench_flow`,
`bench_idle`, `bench_fanin` and `bench_graph` (Cargo.toml). The baselines are written against a messaging
library, not against trame: `barrier.c`, `flow.c` (the ring) and `graph.c`, in the multi-process backend's
`lab/baselines/` (its README). The experiments `round`, `flow`, `idle`, `fanin`, `graph`, `step` and
`latency` (`lab/<name>/README.md`) run them: `nix run .#round-<backend>-s1`.

Every trame worker times itself on its own clock and sends one report to the leader. The leader prints
one JSON line per worker, then an outcomes line. Durations are never compared across workers, since
clocks have unknown offsets (INTERFACE.md 5.1).

## What each benchmark measures

- **round** (`W ROUNDS`): the time for ROUNDS rounds of messages through the leader, after one warm-up
  round. The worker model has no barrier, and a round is the nearest measurement: the leader sends `Go`
  to each of the W workers, each worker answers `Ack`, and the leader sends the next `Go` when all W acks
  are in. A worker's clock runs from its first ack (the warm-up's) to its last (round ROUNDS), so its span
  is ROUNDS full rounds. It was `barrier` (ROUNDS `trame::barrier()` calls) before the barrier left the
  model; the old name and its data are not comparable. The baseline is still ROUNDS barrier calls of the
  messaging library, with one warm-up (`barrier.c`, run as the barrier baseline). The two do not measure one
  thing: a barrier is W ranks agreeing among themselves, a round is W workers agreeing through the leader (W + 1
  agents, 2W messages a round). The comparison is the cost of that choice, not of equal work.
- **ring** (`W STEPS SIZE CAP`): W >= 3 workers in a ring. Each step, a worker sends one SIZE-byte
  message to each neighbour and receives one from each. The clock starts after 10 warm-up steps. The
  baseline does the same with nonblocking receives, nonblocking sends and a wait-all. The trame program waits on
  full routes (capacity CAP). The baseline has no capacity, so it has no CAP.
- **idle** (`W SECONDS`): every worker waits for SECONDS with nothing sent to it. The wall time is the
  wait, and the cost is CPU, which the runner reads per rank from GNU time.
- **fanin** (`W MSGS`): every worker sends MSGS 64-byte messages to the leader. The leader's receive
  time is the time from its first `recv` to its last. Each worker reports its own send span.
- **graph** (`W STEPS SIZE DEGREE ITERS SEED`): the proxy benchmark (`src/backends/README.md`, Benchmarks). Each
  step: ITERS xorshift rounds, one SIZE-byte message to each of DEGREE out-neighbours on a seeded circulant
  graph, one from each in-neighbour. The baseline `graph.c` does the same with nonblocking receives, nonblocking
  sends and a wait-all, and must print the same checksum per rank. `graph.rs` and `graph_proc.rs` (the per-size
  process, a macro) are its sources. The generator and recurrence are in `graph_net.rs`, which the C file repeats.
  Each worker's JSON line has `ns` (STEPS steps), `compute_ns` (the recurrence alone) and `checksum`: the xorshift
  state XOR the sum of every frame received, so it covers the graph, not only the recurrence. It is a u64, stored
  as text in the dataset.
  W must exceed DEGREE, else bench_graph exits 2, and the lab skips such a job and records nothing.

Two measurement caveats are part of the design:

- The fanin leader times itself with `std::time::Instant`, because `trame::now` panics outside a
  worker. That clock is outside the spec's contract: it is a host measurement.
- The ring baseline runs one barrier before its timed steps, and the trame ring does not.
  The two rings therefore start their clocks differently.

## What the numbers cannot show

The claims each benchmark can support, with their thresholds, are in the experiment's README; this is what no run here shows.

- Load. Another session's jobs share the machine. Repeated runs of one configuration can differ widely (no run in the tree
  measures how widely), so a single run supports no scaling claim. Each job records `/proc/loadavg` at its start (the `*_config`
  dataset, source `loadavg`).
- Nodes beyond those run. The round and idle claims about the one-node path say nothing about N nodes until those runs exist.
- Oversubscription. The launch may start more workers than cores (`per` is set by hand in each experiment and not checked
  against the machine), and ranks then share cores. Read the `cpu` dataset before reading a timing.
- Degree. The graph benchmark uses DEGREE 8, which needs W >= 9: a job with fewer workers records nothing.
- The reference backend. Its timings are scheduler timings and are not comparable with a multi-process backend's.
