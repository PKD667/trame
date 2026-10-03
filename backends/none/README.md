# none

One worker and its leader in one process. The default build supplies the whole interface without
MPI or GPU dependencies; `cargo test` uses it.

Other worker geometries are refused. A send to itself answers `Closed`, and any other address, a
`Remote` one included, is `Invalid(RankOutsideJob)`: a launch of one host has no link. Receives
produce no frame.

Local execution and the shared-state primitives work. The leader's segment is a process-local copy
that the worker, in the same process, attaches by address, so `attach` cannot validate the handle
and relies on its `unsafe` promise. It does not check revision reuse; the leader must not publish a
live revision twice. Conformance evidence applies to this single-worker deployment.
