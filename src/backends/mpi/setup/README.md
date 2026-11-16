# setup

From `mpirun` to a launched run: what every rank agrees on, the window it maps, and how the process ends.

| file | holds |
|---|---|
| `open.rs` | `agree`, `launch`, `open`: agreement between ranks, then the window and each rank's `Role` |
| `plan.rs` | the exact encoding of a plan, the route table, `inbox` |
| `shape.rs` | the launch checks of the transport, and the layout they yield |
| `refusal.rs` | why a launch is refused |
| `adopt.rs` | the node's builder puts the states and pool in the segment; the other ranks adopt them |
| `state.rs` | this process's state (world, entries, endpoint, host) and `finalize` |

(The directory is not `launch/`: that is the launcher script of every backend.)

- Every refusal is decided by a collective reduction, so every rank of a host sees it (L2). `agree` runs before
  `open`, and `Backend::agree` also runs the hosts' route comparison (`transport/cross/connect.rs`).
- Only the rank's main thread touches `state.rs`. A host that calls the backend from threads it spawned is
  outside the backend's contract: the global is not synchronised.
- **Finalization.** A worker records its outcome in the window and wakes the leader before it calls
  `MPI_Finalize`; its result is final by then. The leader calls `MPI_Finalize` inside `join`, after it has read
  every outcome, and an `atexit` hook finalizes a rank that exits without `join` (a refusal, a dropped run).
  A worker that finalizes early waits in `MPI_Finalize` until the leader does.
- `TRAME_PROBE=ranks` makes a program print `trame-ranks N` and stop before any collective (`backend.rs`,
  `agree`); the launcher uses it to ask for the rank count.
- Open MPI may print `osc_ucx_component` errors on stderr on some machines. They come from Open MPI.
