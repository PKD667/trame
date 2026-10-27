# Scaling

trame exists so that a program states its algorithm and the runtime carries its messages. A program
scales when the runtime adds no cost that grows with the size of the run. This file defines that, lists
where the MPI backend breaks it today, and states what is measured to show it holds.

## What perfect scaling means here

No runtime makes communication free: the network has a latency and a bandwidth, and a program that
waits for its neighbours every step pays them. That cost belongs to the algorithm and the machine.
trame's part is what it adds on top. Two definitions:

- **Floor.** `T_mpi(N)`: the time per step of a hand-written MPI program (nonblocking sends and
  receives, `MPI_Waitall`) that does the same work over the same graph on the same N nodes. It is the
  cost a scientist pays when they write the communication themselves.
- **Overhead.** `T_trame(N) / T_mpi(N)`, at the same N, workers per node, graph, message size and
  compute per step.

trame scales perfectly when its overhead is bounded by a constant independent of N, and that constant
is close to 1. Equivalently, its weak-scaling curve `T_trame(N) / T_trame(1)` tracks the floor's curve
`T_mpi(N) / T_mpi(1)`. A program written for trame then scales exactly as well as the same program
written against MPI by hand, and no better: the network is not trame's to fix.

The structural condition behind that bound:

> **Locality.** Every operation on a path executed per frame, per wake or per barrier touches O(d)
> words, where d is the degree of the agent or node that runs it (its own routes and its own workers).
> No such path iterates over all workers W, all routes R or all nodes N.

A loop over W or R that runs once per run (launch, `join`, finalize) is allowed. The barrier is the
one operation that must involve every node; its cost is allowed to grow, but only as log N in depth
and O(k) per node, for a fixed fan-out k.

## Where the backend breaks locality today

| path | file | runs | cost today | after |
|---|---|---|---|---|
| `out` route scan | `relay_routes.rs` | every relay wake | O(R) routes of the whole layout | O(routes with one end here) |
| `out` frame buffer | `relay_routes.rs` | every outbound route, every wake | one allocation of `size` bytes | one scratch buffer per thread |
| `out` control scan | `relay_control.rs` | every relay wake | O(W) | O(local workers) |
| `bump_local` | `relay_in.rs` | every release, exit, abort | O(W) checks | O(local agents) |
| barrier, across nodes | `relay_control.rs`, `relay_in.rs` | every barrier | star: root sends and receives N - 1 | k-ary tree: depth 2 log_k N, root O(k) |
| barrier, one node | `gate.rs` | every barrier | the last arriver bumps W workers | unchanged: W here is the local count, bounded by cores |

The first row is the one that matters. A ring of W = 1024 workers with 8 routes each makes the relay
read 8192 route records on every wake, whichever node it runs on, so its cost per frame grows with
N under weak scaling. NODES.md's table already states the intended cost ("routes with one end on the
node"); the code does not meet it.

The barrier is not fixed by argument alone. A star costs the root about 2(N - 1) message handlings
and two network latencies; a tree costs 2 log_k N latencies and O(k) handlings per node. At N = 32
with a 10 µs latency the two are close, so the fan-out k is a constant in `nodes.rs`, chosen from
the measured barrier curve (k >= N - 1 is the star). One mechanism, one constant, set by data.

## What does not change

- **Futex waits.** A worker waiting for a remote frame sleeps; the relay's `in` thread wakes it.
  A sleep and a wake cost a few microseconds that a polling MPI program does not pay. This is a choice
  (no spinning workers), and its price is part of the overhead constant. The proxy benchmark below
  measures it.
- **The relay core.** `in` blocks in `MPI_Recv`, which spins: one core per node, measured by the
  `cpu` dataset.
- **One sender per node.** All cross-node frames of a node leave through one thread. This is a cost
  (serialisation) and a gain (frames to the same node share one MPI message: at most N - 1 messages
  per wake, whatever the number of workers). Which one dominates depends on message size and degree;
  the proxy benchmark sweeps both.

## The proxy benchmark

`scale_graph` is the shape of a simulation step, not of a microbenchmark. Each of W workers owns a
block of work. Each step it computes for a fixed number of iterations, sends one message to each of
its D out-neighbours, receives one from each of its D in-neighbours, and starts the next step.

- **Graph.** A circulant graph: D distinct offsets drawn from 1..W-1 by a seeded generator, the same
  in Rust and in C; worker w sends to (w + o) mod W for each offset o. In-degree equals out-degree
  equals D, and with offsets drawn over the whole range most edges cross nodes: the worst placement.
- **Compute.** ITERS rounds of a fixed integer recurrence (xorshift), so the work is identical in both
  implementations and does not depend on a clock. Each worker records its compute time separately.
- **Weak scaling.** ITERS and the workers per node are fixed; N grows.
- **Strong scaling.** ITERS x W is fixed; N grows, and the compute per worker shrinks.
- **Baseline.** `baseline/graph.c` does the same with `MPI_Irecv`, `MPI_Isend`, `MPI_Waitall`.
- **Result.** Each worker times STEPS steps after a warm-up on its own clock; the runner records every
  worker's time. A step's cost is set by the slowest worker, so claims use the maximum and the median.

## Claims

Stated before measuring, judged at 0.99 by the `scale` analysis (experiments/README.md):

- **Overhead.** In `graph`, weak scaling, D = 8, SIZE in {64, 1024}, ITERS set so compute is about
  100 µs per step: trame's maximum time per step is at most 1.10 x the baseline's, at every N.
- **Tracking.** Same jobs: `T_trame(N) / T_trame(1)` is at most 1.05 x `T_mpi(N) / T_mpi(1)`, at every N.
- **Locality.** The relay rank's CPU time per frame relayed does not grow with N (slope test).
- **Strong scaling.** trame's speedup `T(1) / T(N)` is at least 0.9 x the baseline's, for every N at
  which the baseline's parallel efficiency is at least 0.5.

A contradicted claim stays in the report with the profile that explains it.
