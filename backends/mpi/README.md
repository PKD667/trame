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

`leader::done` is not implemented yet: `Leader`'s `Drop` coordinates shutdown instead.
