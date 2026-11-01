# Small cross-node frames over RMA

Status: design, written before the code. `TRAME_MPI_SMALL=rma` selects it; the default is `p2p`,
which is today's transport and must stay byte for byte as it is.

## What changes

A **small route** is a route with `from` and `to` on different nodes, `lossy` false, and
`size <= SMALL_BYTES` (4096, a constant in `relay_rma.rs`). Only small routes take the RMA path. A
route keeps its path for its whole life, so no route mixes RMA and p2p frames. Same-node routes,
lossy routes and large routes stay on p2p. Credits, barriers, results, aborts and END stay on p2p.

Where it plugs in:

- `open.rs`: after the node table and the window, every rank computes the same lane list from the
  topology and the node table. If the list is not empty, every rank calls `Ring::raw` on the world
  communicator (collective). Ranks agree on success, and a failure is refused on all ranks.
- `relay_out.rs` `frames()`: a frame of a small route is sent with `Ring::send` instead of being
  queued in `bufs`. The frame is `route (u32) | index (u64) | payload`, 12 bytes of head.
- `relay_rma.rs` (new): the rma-in thread. It polls the incoming lanes with `Ring::poll` and applies
  each frame with the code `relay_in.rs` already has (`put`, store P, bump the receiver).
- `relay.rs`: the rma-in thread is started and joined with the other two, only in rma mode.

Lanes are per ordered node pair, between the relay ranks. Depth is the sum of the capacities of the
small routes on the lane. Capacity is the largest frame on the lane (12 + size).

## Why the lane cannot overwrite or lap

Let `held(r) = P_A(r) - C_A(r)` on the sending node (spec 5.2). A worker's send needs
`held(r) < cap(r)`, so before the relay pushes frame `i` of route `r`, `held(r) <= cap(r) - 1`.

- `R_r` = frames of `r` that the receiving relay has read from the lane. `R_r >= C_B(r) >= C_A(r)`,
  because the receiver stores `C_B` only after it has written the frame into the route ring, and
  the credit it sends carries a value `C_B` had at that time.
- So unread frames on the lane, summed over lane routes, are at most `sum_r (held(r)) <= depth - 1`
  before the push. The slot the push overwrites was read. Its read is ordered before the receiver's
  credit, and the credit before the sender's load of `C_A`, so the overwrite is after the read.
- The route ring on the receiver cannot overflow: `P_B - C_B <= P_A - C_A`, both sides move at most
  as the spec allows.

So `Ring::raw` never laps, `Ring::send` never refuses, and `lost()` must stay 0. The lane's `Full`
is never answered. `Full` on a route is still decided only by the sender's `held` (Full, spec 5.2).

## FIFO and the spec

- Route order: the relay pushes a route's frames in index order, the lane is FIFO for one origin
  (each `Ring::send` completes its Put and flush before the next), and rma-in applies in lane order.
  One route, one lane, one path: FIFO per route holds (spec 5.2, Queues).
- Frames against control records: a frame on the lane may arrive after a release, a result or
  END. The spec lets a frame arrive late (its arrival time is chosen by the environment). The p2p
  path already reorders a frame against a barrier record within one scan, so this is not new.
  If a test shows a worker that cannot receive a frame it was owed, that is a stop (below).
- Late frames after END: rma-in stops when `in` has seen END. A frame still on a lane then is not
  applied. Every worker has returned and the leader has joined, so nothing can observe it, as on
  p2p. This is stated, not argued from the spec.
- Lossy routes and Overwrite/Drop: not on this path. Nothing changes for them.

## Progress

Puts must land without the receiver making an MPI call, because rma-in makes none. Measured here:
Open MPI selects `osc sm`, and the probe in `/tmp/rma-probe` (not in the repo) gets 8 of 8 frames by
polling memory with no MPI call. `osc rdma` gives the same on one node. Across real nodes the
one-sided component is `rdma` or `ucx`, and whether a Put needs target progress there is not
tested. If it does, rma-in stalls. The fix would be an `MPI_Iprobe` in rma-in's idle loop. Not added
yet, because it costs MPI lock time on a path that has no measurement.

## Cost, stated before measuring

- The sender pays one `MPI_Put` and one `MPI_Win_flush` per frame, on the out thread (mpi-rma
  `Window::put`, unchanged). That is a round trip per frame to the target, not a batch.
- Each receiving node gets one more thread, which spins when idle. The p2p `in` already spins
  inside `MPI_Recv`, so the node's cost is about the same.
- A lane's slot is sized to its largest frame. Mixed sizes on one lane waste space.
- Lane memory is depth times slot, per receiving relay. A route with a large cap makes a deep lane.
- On this machine both paths are a memory copy in shared memory. I expect RMA to be no faster here,
  possibly slower. The case for RMA is a network where `MPI_Send` costs a message round trip. That
  is not measured by this machine.

## Stop, and report, if

- Any `fifo` or `overwrite` check fails in rma mode, or `lost()` is not 0. Do not weaken a test.
- A launch that cannot use RMA is not refused silently: the refusal names the reason, once.
- The latency table shows rma slower here. Report the numbers. Do not change the default from them.
- A choice of `SMALL_BYTES` is taken from a measurement here. It stays a constant until a
  measurement says otherwise.

## Setting

`TRAME_MPI_SMALL` is unset (p2p) or `p2p` or `rma`. Any other value, including an empty string, is
refused on every rank before any split, with the value named, as `TRAME_MPI_NODE_RANKS` is. With one
node there are no small cross-node routes, so `rma` launches no `Ring` and makes no extra MPI call:
it is the p2p launch.
