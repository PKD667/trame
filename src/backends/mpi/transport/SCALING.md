# Scaling

trame exists so that a program states its algorithm and the runtime carries its messages. A program
scales when the runtime adds no cost that grows with the size of the run. This file defines that, lists
where the MPI backend breaks it, and states what is measured to show it holds.

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

The structural condition behind that bound (Locality):

> **Locality.** Every operation on a path executed per frame or per wake touches O(d)
> words, where d is the degree of the agent or node that runs it (its own routes and its own workers).
> No such path iterates over all workers W, all routes R or all nodes N.

A loop over W or R that runs once per run (launch, `join`, finalize) is allowed.

## Locality in the tree

The condition is not met by every path today, and nothing measures how much it costs.

- `Transport::poll_lanes` and `has_lanes` (`mod.rs`) walk `lanes`, which holds one slot per route of the host
  (`cross/mod.rs`, `lanes`), not one per lane of this endpoint. A worker with a lane polls it in `idle`
  (`roles/worker.rs`), so each poll costs O(R of the host), not O(d).
- `Lane::poll` itself tests only the requests of its own lane: O(cap).
- The earlier text of this file described a relay thread's scans (`relay_routes.rs`, `relay_control.rs`) and the
  cost of its `in` core and its single sender. The tree has no relay, so none of that holds, and the claim built
  on it (Locality, below) has no subject until it is restated for lane polling.

## What does not change

- A worker with a lane does not sleep, so its cores spin while it waits for a remote frame. A worker without one
  sleeps on its futex, and a sleep and a wake cost a few microseconds that a polling MPI program does not pay.
  That price is part of the overhead constant, and the proxy benchmark measures it.

## The proxy benchmark

`bench_graph` is the shape of a simulation step, not of a microbenchmark. Each of W workers owns a
block of work. Each step it computes for a fixed number of iterations, sends one message to each of
its D out-neighbours, receives one from each of its D in-neighbours, and starts the next step.

- **Graph.** A circulant graph: D distinct offsets drawn from 1..W-1 by a seeded generator, the same
  in Rust and in C. Worker w sends to (w + o) mod W for each offset o. In-degree equals out-degree
  equals D, and with offsets drawn over the whole range most edges cross nodes: the worst placement.
- **Compute.** ITERS rounds of a fixed integer recurrence (xorshift), so the work is identical in both
  implementations and does not depend on a clock. Each worker records its compute time separately.
- **Weak scaling.** ITERS and the workers per node are fixed, and N grows.
- **Strong scaling.** ITERS x W is fixed. N grows, and the compute per worker shrinks.
- **Baseline.** `bench/graph.c` does the same with `MPI_Irecv`, `MPI_Isend`, `MPI_Waitall`.
- **Result.** Each worker times STEPS steps after a warm-up on its own clock, and the runner records every
  worker's time. A step's cost is set by the slowest worker, so claims use the maximum and the median.

## Claims

Stated before measuring, judged at 0.99 by the `scale` analysis (lab/README.md):

- **Overhead.** In `graph`, weak scaling, D = 8, SIZE in {64, 1024}, ITERS set so compute is about
  100 µs per step: trame's maximum time per step is at most 1.10 x the baseline's, at every N.
- **Tracking.** Same jobs: `T_trame(N) / T_trame(1)` is at most 1.05 x `T_mpi(N) / T_mpi(1)`, at every N.
- **Locality.** Withdrawn with the relay (see Locality in the tree). No experiment tests it.
- **Strong scaling.** trame's speedup `T(1) / T(N)` is at least 0.9 x the baseline's, for every N at
  which the baseline's parallel efficiency is at least 0.5.

A contradicted claim stays in the report with the profile that explains it.
