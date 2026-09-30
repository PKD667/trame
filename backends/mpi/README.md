# mpi

One MPI world per launch. Batches travel as MPI messages; local lanes are RMA rings, acknowledged
unless the `lossy` feature selects overwrite-on-full rings. The choice is a compile-time feature so
every rank of a launch agrees on it by construction. A frame to another host crosses on an MPI link,
which never loses a frame even under `lossy`.

A deployment is one host. Its leader and workers must share one POSIX shared-memory namespace, and
all launch participants agree on admission before any bridge is created; a deployment spread over
hosts is refused as `InconsistentLaunch`.

`TRAME_MACHINEFILE=allocation.nodes bash trame/scripts/conform.sh mpi-stage OUT` derives one
deployment per unique allocated host, with four workers and one leader each. `MPIRUN` supplies the
site launcher and remote agent. Main and pressure preserve the host-local claims; F1 exchanges
messages and lanes between every worker pair on different hosts. A successful admission alone is
not evidence of cross-host delivery.

Segments are named POSIX objects, `/dev/shm/trame-<pid>-<rev>`. Publish reserves the whole object
before copying, so an exhausted `/dev/shm` is a refusal carrying its errno (`ENOSPC`), not a
`SIGBUS`. Publishing a revision the leader holds live is refused (`EEXIST`). A process that dies
before retiring leaves its objects behind for the operator to remove.

Workers call `done`; leaders call the method `Leader::done`. Dropping either handle is not a
shutdown operation. Every application first agrees that it has finished, then each rank detaches
its buffered-send storage on a helper thread while the calling thread drains and discards unread
Message/link/leader frames. A second job agreement keeps all receivers draining until all buffers
have detached. Only then are communicators/windows freed and MPI finalized. This uses the
`MPI_THREAD_MULTIPLE` level entry already requires: no silent application receiver must receive,
and no application frame is delivered by shutdown. The existing M5 pressure launch falsifies this
contract if `done` waits for application receipt or traffic drains before the send buffer fills.
