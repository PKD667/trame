# Credit return without sender progress (Open MPI 5.0.11, MIST, 2 ranks, cap 4, K 200)

Command: `bash run.sh out` (matrix); `mpirun --oversubscribe -n 2 [--mca ...] ./out/progress MODE 4 K STALL_MS DIR`
Sender waits for credit with plain loads (no MPI call), STALL_MS=200 before one MPI_Iprobe.
analyze.py prints full_without_cap_outstanding (must be 0) and stall_events.

| run | osc selected | stalls | full w/o cap outstanding | credit latency median (ns) |
|---|---|---|---|---|
| shm (MPI_Win_allocate_shared + atomics) | sm | 0 | 0 | 241 |
| rma: Fetch_and_op+flush, osc sm | sm | 0 | 0 | 100 |
| rma: Fetch_and_op+flush, osc rdma | rdma | 0 | 0 | -70 (negative: see note) |
| rma, osc rdma, btl tcp,self | rdma | 0 | 0 | -251 (note) |
| acc: Accumulate, no flush, osc sm | sm | 0 | 0 | 441 |
| acc: Accumulate, no flush, osc rdma | rdma | 0 | 0 | 165 |
| acc, osc rdma, btl tcp,self (1 Full only) | rdma | 0 | 0 | 5731 (n=1) |
| aop: MPI_REPLACE on derived 2xu64, osc rdma | rdma | 0 | 0 | -360 (note) |
| aop, osc rdma, btl tcp,self | rdma | 0 | 0 | -360 (note) |

Note: negative values mean the sender saw the credit before the receiver logged its ack-return
time. The receiver logs after its store/flush, so this is the measurement window, not a bug.

Conclusion: no configuration on one node made credit wait for sender progress. Casper's case
(software active messages executed only inside the target's MPI progress) did NOT reproduce here,
because the single node uses shared-memory atomics or direct copies. It is not ruled out for
cross-node or other OSC paths, and this machine cannot reach them. (`aop` used MPI_REPLACE with a
derived type because MPI_Accumulate rejects user-defined ops, MPI_ERR_OP.)

The mechanism is chosen on the spec's linearization argument, not on this measurement: Full must be
linearized at a read of the consumer's head that already includes every completed consume. A
receiver-written word in shared memory, read by the sender with an acquire load, satisfies that with
no MPI call at credit time. An RMA ack satisfies it only after the flush completes, which blocks the
receiver on sender progress in the case where that matters.
