# none

The degenerate deployment: one worker, its leader, and no transport peer. It exists so a build can
link the whole surface with no MPI and no GPU, which is what `cargo test` runs on.

It refuses any other worker geometry. A send to itself answers `Closed`, and any other address, a
`Remote` one included, is `Invalid(RankOutsideJob)`: a launch of one host has no link. Receives
produce no frame.

Local execution and the shared-state primitives work. The leader's segment is a process-local copy
that the worker, in the same process, attaches by address, so `attach` cannot validate the handle
and relies on its `unsafe` promise. It does not check revision reuse; the leader must not publish a
live revision twice. Nothing here is evidence for communicating deployments.
