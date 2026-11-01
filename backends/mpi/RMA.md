# Small cross-node frames over RMA

`TRAME_MPI_SMALL=rma` selects this path. The default is `p2p`, today's transport, unchanged.

## What changes

A **small route** has `from` and `to` on different nodes, `lossy` false, and `size <= SMALL_BYTES`
(4096, in `small.rs`). Only small routes take the RMA path. A route keeps its path for its whole life.
Same-node routes, lossy routes and large routes stay on p2p. Credits, barriers, results, aborts and END
stay on p2p.

Where it plugs in:

- `open.rs`: after the window, every rank computes the same lane list from the topology and the node table
  (`small.rs` `lanes`). If the list is not empty, every rank calls `Ring::raw` on the world communicator.
  The call is collective. A failure on any rank is a refusal on every rank.
- `relay_routes.rs` `frames()`: a small route's frames go to `Out::send_lane` (`relay_rma.rs`), one
  `Ring::send` per frame, instead of being queued for MPI_Send. The lane message is a 12-byte head
  (route u32, index u64) and the payload.
- `relay_rma.rs` `run_in`: the `rma-in` thread polls the lanes that end here and applies each frame with
  `relay_in.rs` `apply_frame`, the code the p2p `in` thread uses.
- `relay.rs`: `rma-in` is started and joined with the other threads, only when the launch has lanes.

A lane is per ordered node pair. Its depth is the sum of the capacities of the small routes on it. Its
slot is the largest frame on it plus the head.

## Why a lane cannot overwrite or lap

Let `held(r) = P_A(r) - C_A(r)` on the sending node (spec 5.2). A worker's send needs `held(r) < cap(r)`.

- `R_r` = frames of `r` that the receiving relay has read from the lane. The receiver writes a frame into
  the route ring before it counts it consumed, so `R_r >= C_B(r) >= C_A(r)`. C_A is a credit the sender has
  seen, and a credit is never ahead of C_B.
- Unread frames on the lane are `P_A(r) - R_r <= P_A(r) - C_A(r) = held(r) <= cap(r) - 1`, per route, when
  the relay pushes. Summed over the lane's routes that is below the depth. The slot a push writes was read.
- The receiver's route ring cannot overflow for the same reason as on p2p.

So `Ring::raw` does not lap, `Ring::send` does not refuse, and `lost()` must be 0. The check in
`relay_rma.rs` asserts it. This argument holds if the relay pushes each frame once, in index order, which
`relay_routes.rs` does.

## FIFO and the spec

- One route, one lane, one origin: a `Ring::send` completes (Put and flush) before the next one starts, and
  `rma-in` applies in lane order. FIFO per route holds (spec 5.2, Queues).
- A frame can reach the receiver after a control record (release, result, END), as on p2p. The spec lets a
  frame arrive late. Nothing in the code waits for a frame the receiver is not owed.
- Stop order at END: `out` makes its last scan and sets `out_done`. `rma-in` stops when `out_done` is set and
  the lanes are empty. So every frame `out` sent is applied before the thread stops, and no frame is sent
  after the lane stopped being read.
- Lossy routes, Overwrite and Drop: not on this path. Nothing changes for them.

## Progress

`rma-in` makes no MPI call, so a Put must land without target progress. On this machine Open MPI selects
`osc sm`, and a probe in `/tmp/rma-probe` (outside the repo) gets 8 of 8 frames by polling memory. Across
nodes the component would be `rdma` or `ucx`. Whether a Put there needs target progress is not tested here.
If it does, `rma-in` stalls on a real network. That is the largest open risk.

## Measured here

`backends/mpi/tests/latency-sizes.tsv`, made by `latency_sizes.sh`: median and p99 round trip per probe size,
3 runs per cell, 6636 samples per run, one machine. Summary (median over 3 runs, range in brackets, ns):

    bytes  split-one-machine p2p      split-one-machine rma        same-node p2p   same-node rma
      64   26079 [25999..26881]       24105 [24066..27652]           470            1042
     256   30006 [25679..30578]       29015 [26219..29305]           932             842
    1024   27983 [25268..30978]       32822 [30537..33383]          4288            2224
    4096   51206 [49293..52899]       62257 [57839..62397]          7094           14347
    8192*  93947 [85301..103685]      75762 [74911..80822]         26239           28664

`*` 8192 bytes is over SMALL_BYTES: both labels are p2p, so this row is a control for the run-to-run spread.

What the table says:
- A split costs about 25 us on either path, against under 1 us on one node. The cost is the relay hop
  (wake, scan, the second thread), and the RMA lane does not remove it.
- Below 4 KiB the two paths overlap within their spreads. 4096 looks about 20% slower on rma. The 8192 control
  differs by about 20% with the same code on both sides, so that size of gap is inside this machine's noise.
  No slowdown is established.
- The p99 is 70 us to 1.4 ms on the split rows and does not improve on rma.

Command: `TRAME_MPI_NODE_RANKS=1 TRAME_MPI_SMALL=<p2p|rma> mpirun --oversubscribe -n 3 latency --bytes <B>`,
Open MPI 5.0.11, AMD Ryzen 5 PRO 4650U, 12 CPUs, Linux 7.2.7. The `route` column says `node-split-one-machine`:
a split on one machine is not a cross-node route.

## Cost, stated before measuring

- Sender: one `MPI_Put` and one `MPI_Win_flush` per frame (mpi-rma `Window::put` and `Ring::send`). A round
  trip per frame, not a batch. Not measured on a network.
- Receiver: one more thread, which spins while idle and yields after 1000 empty polls.
- Lane memory: depth times slot, per receiving relay. A large cap makes a deep lane.

## Stop, and report, if

- Any check fails in rma mode, or `lost()` is not 0. Do not weaken a test.
- The latency table shows rma slower beyond the run-to-run spread. Report the numbers. Do not change the
  default from them. `SMALL_BYTES` stays a constant until a measurement on a network says otherwise.

## Setting

`TRAME_MPI_SMALL` unset or `p2p`: p2p. `rma`: this path. Any other value, including empty, is refused on every
rank before any split, with the reason `TRAME_MPI_SMALL is not p2p or rma` and the value. A `Ring::raw` failure
is refused on every rank with its error, and no worker starts. With one node there are no lanes and no extra
MPI call: `rma` is then the p2p launch.

## Not verified

- Real network, Grid'5000, UCX, and any Put that needs target progress.
- Behaviour of `Ring::raw` when it fails on some ranks and not others (it is collective inside, and I have not
  tested a partial failure).
- Whether the relay hop, not the transport, is the cost on a network. The measurement here says it is on one
  machine only.
