# nv

One CUDA warp per rank, one GPU per leader, in the launch's single CUDA context. The same source
builds as a host model (feature `nv`) and for the device (feature `cuda`), compiled through the
cuda-oxide fork. Its build tool is `cargo nv` (`cargo/`, package `nv-cargo`).

Scheduling mirrors the host lowering: one lane-zero owner per worker runs invocations in list order.

The following operations refuse with `BackendFault::Unimplemented`:

- **Links.** A `Remote` address naming a worker of another host. Multi-host GPU is deferred.
- **Segments.** `leader::publish` and `attach` refuse, so no `Published` or `Shared` value exists
  and `bytes`, `detach` and `retire` cannot be reached. `Published` is nv's own uninhabited type.
  In S1 conformance the cell reads `UNIMPLEMENTED` only when the leader's publish and all four
  workers' attach refuse exactly so. This records refusal coverage; segment transfer remains unverified.

The intended segment is a leader device allocation filled by host-to-device copy on a pre-created
stream, its handle token the device pointer. cuda-core allocation and its freeing `Drop` are
synchronous, so whether they progress beside a persistent kernel is an open hardware question.
