# Scaling benchmarks

Five trame programs and three plain-MPI baselines. The trame programs are examples `scale_barrier`,
`scale_ring`, `scale_idle`, `scale_fanin`, `scale_graph` (Cargo.toml). The baselines are `baseline/barrier.c`,
`baseline/ring.c` and `baseline/graph.c`. The lab experiments `scale-local` and `scale-nN` run them (flake.nix,
`experiments/scale.py`): `nix run .#scale-local`.

Every trame worker times itself on its own clock and sends one report to the leader. The leader prints
one JSON line per worker, then an outcomes line. Durations are never compared across workers, since
clocks have unknown offsets (spec.md 5.1).

## What each benchmark measures

- **barrier** (`W ROUNDS`): the time for ROUNDS `trame::barrier()` calls, after one warm-up barrier.
  The baseline is ROUNDS `MPI_Barrier` calls, with the same warm-up.
- **ring** (`W STEPS SIZE CAP`): W >= 3 workers in a ring. Each step, a worker sends one SIZE-byte
  message to each neighbour and receives one from each. The clock starts after 10 warm-up steps. The
  baseline does the same with `MPI_Irecv`, `MPI_Isend` and `MPI_Waitall`. The trame program waits on
  full routes (capacity CAP); MPI has no capacity, so its baseline has no CAP.
- **idle** (`W SECONDS`): every worker waits for SECONDS with nothing sent to it. The wall time is the
  wait; the cost is CPU, which the runner reads per rank from GNU time.
- **fanin** (`W MSGS`): every worker sends MSGS 64-byte messages to the leader. The leader's receive
  time is the time from its first `recv` to its last. Each worker reports its own send span.
- **graph** (`W STEPS SIZE DEGREE ITERS SEED`): the proxy benchmark of `backends/mpi/SCALING.md` ('The proxy
  benchmark' is its spec). Each step: ITERS xorshift rounds, one SIZE-byte message to each of DEGREE
  out-neighbours on a seeded circulant graph, one from each in-neighbour. The baseline `baseline/graph.c`
  (`mpi_graph`) does the same with `MPI_Irecv`, `MPI_Isend`, `MPI_Waitall`, and must print the same
  checksum per rank. `graph.rs` and `graph_proc.rs` (the per-size process, a macro) are its sources; the
  generator and recurrence are in `graph_net.rs`, which the C file repeats. Each worker's JSON line has
  `ns` (STEPS steps), `compute_ns` (the recurrence alone) and `checksum`: the xorshift state XOR the sum of every frame received,
  so it covers the graph, not only the recurrence. It is a u64, stored as text in the dataset.
  W must exceed DEGREE, else scale_graph exits 2; the lab skips such a job and records nothing.

Two measurement caveats are part of the design:

- The fanin leader times itself with `std::time::Instant`, because `trame::now` panics outside a
  worker. That clock is outside the spec's contract; it is a host measurement.
- The MPI ring baseline runs one `MPI_Barrier` before its timed steps, and the trame ring does not.
  The two rings therefore start their clocks differently.

## What each benchmark can support

- **Weak scaling** (barrier, ring, fanin): per-worker time stays flat as W grows, with the work per
  worker fixed. Flat per-worker time is the claim that the runtime adds no cost that grows with W.
  *Refuted by*: per-worker time that rises steadily with W on an otherwise idle machine, at one node.
- **Barrier**: cost grows with W and with the number of nodes. Only one node is measured here, so
  the claim is about the one-node path. *Refuted by*: a per-barrier cost that grows faster than
  linearly in W at fixed load, or a trame barrier slower than `MPI_Barrier` at the same W.
- **Ring vs. baseline**: trame's ring is no slower than the plain-MPI ring at the same W and SIZE.
  *Refuted by*: a trame ring median that is consistently above the baseline's, with both runs at
  low load. The trame ring also pays for its checked build (`--cfg trame_checked`), which the baseline
  does not.
- **Idle**: a waiting worker costs almost no CPU, so CPU seconds per idle worker is near zero. A
  rank also pays for MPI startup and finalization, so a control run with SECONDS=0 is needed to read
  the wait cost off the total. *Refuted by*: CPU seconds that grow with SECONDS, rather than staying
  at the SECONDS=0 level. With nodes > 1, the relay (one core per node) is expected to be the only
  steady CPU cost; the single-node matrix cannot show it.
- **Fanin**: the leader is a single sink, so its receive rate sets a ceiling on the total message
  rate. The claim is the ceiling, not the leader's latency. *Refuted by*: a leader time that grows
  faster than the number of messages it receives, at fixed load.

## What the numbers cannot show

- Load. Another session's jobs share the machine in our runs. Repeated runs of one configuration
  differ by a factor of several, so a single run supports no scaling claim. Each job records
  `/proc/loadavg` at its start (dataset `mpi_config`, source `loadavg`); read it before reading a number.
- Nodes. `scale-local` is one node. `scale-nN` (N nodes) is defined in flake.nix, but no locked run is
  quoted here. The barrier and idle claims are about the one-node path until those runs exist.
- Oversubscription. The runner passes `--oversubscribe`, so it would let ranks share fewer cores than
  they have. The graph sweep avoids this by its `fill` rule: workers per node are the physical cores less
  two (`full`) or half of them (`half`), so the ranks of a job do not exceed the physical cores. The
  other benches do not have that rule.
- Degree. The graph claims use DEGREE 8, which needs W >= 9. With `fill=half` on a 6-core host there
  are 3 workers per node, so the graph jobs of small N record nothing (see Graph above).
- The seq backend. Its timings are scheduler timings and are not comparable with the MPI backend.
