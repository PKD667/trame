# roles

What a rank does once launched: rank i < W is worker i, rank W is the leader.

| file | holds |
|---|---|
| `worker.rs` | the worker's operations on its endpoint, under `XPORT`, the rank's transport lock |
| `runner.rs` | a worker rank's run: agents, result, exit. It never returns to the host |
| `leader.rs` | the `Leader` run: send, recv, edit, stop, wait, join |
| `results.rs` | outcome words, the abort record, the start time |
| `detect.rs` | the leaders' deadlock detector |

- A worker rank runs several agents of the reference scheduler in threads. The endpoint is one per rank and
  not `Sync`, so every operation holds `XPORT`; steps still compute in parallel, only their effects serialise.
- An abort reaches a worker at its next effect, not while it computes. The first panic of a node is an
  object `<window>-abort-0`, created exclusively, so one panic is recorded per node.
- **Detector.** The leaders run rounds of a nonblocking allreduce on the peer communicator while they wait, join
  or have ended. A round sums the leaders whose snapshot of their host's words moved, the messages sent and
  those arrived, and the leaders that have not ended. A round with nothing moved and as many arrived as sent
  shows an instant at which no step runs, no message is in flight and no epoch can move.
- A joining leader then ends its workers with `Deadlock`, and a waiting leader panics, as on seq. A leader that
  runs its own code holds the rounds back, so nothing is shown while it does; that is sound, since it may
  still send.
- Each end of a lane counts, in its route's two window words, the messages it sent and those it saw arrive,
  credits included; an ended end keeps observing its posted receives until the final barrier.
- A route that one host declares and the other does not is refused on the declaring host when the other host's
  leader ends, since the ending leader answers every connect still waiting for it.
- When one host aborts, a worker of another host that waits on a lane from it ends with `Deadlock` instead of
  hanging both hosts.
- Models: `src/backends/mpi/proofs/Detect.tla`, `Edit.tla` (the leader's edit handshake).
