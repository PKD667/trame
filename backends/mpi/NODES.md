# Across nodes

How the MPI backend runs workers on several shared-memory nodes. The target is a few thousand
workers: tens to a hundred nodes of 16 to 64 cores.

## Replicas and relays

- **Nodes are devices.** `MPI_Comm_split_type(SHARED)` gives the nodes, numbered by their lowest
  world rank. $\mathrm{dev}(w)$ is the node of worker $w$'s rank. Each node has its own clock,
  `CLOCK_MONOTONIC` with that node's start, and its `ClockId` is the node number, so a reading
  from one node never meets a reading from another without aborting (spec 5.1).
- **Ranks do not change.** W + 1 ranks: ranks `0..W` are workers, rank W is the leader, placed
  wherever `mpirun` puts them.
- **Every node holds a replica of the whole layout.** Each node allocates its own window with the
  layout every rank computes, so every offset means the same thing on every node. `send`, `recv`,
  the barrier and the results run unchanged on each node. A node touches only the lines of its own
  agents and the rings of routes with an end on it.
- **A bump to a remote agent wakes the relay.** `bump(w)` for an agent on another node bumps the
  node's relay epoch instead (`RELAY_EPOCH`, `RELAY_SLEEP`, same sleep protocol as a worker).
- **The relay** runs in the process of the node's lowest world rank, as two threads:
  - `out` sleeps on the relay epoch. When woken it scans the node's outbound routes, credits,
    barrier count, results, exits and abort bit, and sends what is new, one `MPI_Send` per
    destination node per scan, so small frames travel in batches.
  - `in` blocks in `MPI_Recv(ANY_SOURCE)` and applies each record to the local replica, then bumps
    the local agents it concerns.

  Only `out` sends and only `in` receives, so a blocked send never stops a receive, and MPI's
  ordering between one pair of ranks keeps every route FIFO. This needs `MPI_THREAD_MULTIPLE`; a
  multi-node launch without it is refused.
- **The cost.** Open MPI's blocking receive spins in its progress loop, so `in` keeps one core busy
  on every node for the whole run. Workers still sleep in the kernel. The scaling benchmarks measure
  this cost; a socket transport that sleeps would replace `in` without changing anything else.

With one node there is no relay, no thread and no extra MPI call: the backend is the one-node
backend.

## A route between nodes

Route $r$ from $s$ on node A to $v$ on node B, capacity $c$, lossless:

1. $s$ writes A's ring and P as on one node, and bumps $v$, which wakes A's relay.
2. A's `out` keeps a cursor F with $C_A \leq F \leq P_A$, sends frames F to $P_A - 1$ with their
   indices, and advances F. It never moves $C_A$.
3. B's `in` writes each frame into its slot in B's ring, stores $P_B$, and bumps $v$.
4. $v$ receives on B as on one node, which moves $C_B$ and bumps $s$, which wakes B's relay.
5. B's `out` sends $C_B$ as a credit. A's `in` stores it as $C_A$ and bumps $s$.

$P_A - C_A$ is $\mathrm{held}(r)$ (spec 5.2), and A's C moving is **Release**. `Full` is answered
exactly when $\mathrm{held}(r) = c$: a slot consumed on B but not yet credited on A still counts,
as **Release** allows. B's ring cannot overflow: a frame with index $i$ was sent only when
$i < C_A + c \leq C_B + c$. A lossy route has no credits. A's `out` sends the newest frames it can
read; a frame overwritten before it is read is a **Drop**, and B's ring overwrites as on one node.

## Control

The relay of the leader's node is the root.

- **Barrier.** Local workers add to the node's COUNT as on one node. The arrival that brings
  `COUNT mod W` to the node's worker count bumps the relay, and `out` sends `ARRIVED(g)` to the root.
  When all N nodes have arrived, the root reads the leader's deposit once and sends
  `RELEASE(g, number, bytes)` to every node, itself included. Each relay installs that publication
  and adds W minus its local workers to COUNT, which makes it the releaser of its node. Every node
  therefore installs the same publication at the same generation.
- **Results and exits.** `out` sends each new local outcome and each new exit to every other node.
  `in` stores them, so `join` on the leader's node and the barrier's Deadlock check see every worker.
- **Abort.** `out` sends the abort bit once, and `in` sets it locally and wakes every local agent.
  An abort that arrived is not sent again.
- **End.** After `join`, the leader sets END and bumps the relay; the root sends END to every node.
  Each relay stops, and its process calls `MPI_Finalize` only after both threads have stopped.

Not built yet: detecting that nothing can move after `join` (counting waves over the relays). The
one-node backend lacks it as well.

## Testing on one machine

`TRAME_MPI_NODE_RANKS=k` splits each shared-memory node into groups of k consecutive ranks, each
with its own window and relay. It runs the multi-node code on a laptop, and `programs/run.sh` runs
every program with k = 1 and k = 2 as well. It is a test setting and never a default.

## What grows

| quantity | per node | root |
|---|---|---|
| window | the whole layout (only local parts touched) | same |
| messages per barrier | 2 | 2N |
| messages per result | N − 1 | |
| `out` scan | outbound and inbound lossless routes of this node (precomputed) | |
| control scan | local workers | |

The replica is the simple choice and costs address space: a program whose layout the host cannot
map is refused, as on one node, with the size asked for in the reason. A per-node layout is the
change to make if a real program hits that limit.
