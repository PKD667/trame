# trame

`spec.md` is the contract for humans; the crate (`lib.rs` and the macros) is the same contract in
Rust. They change together. Backends live in `backends/<backend>/`; the checker is `driver/`.

| backend | runs on | feature |
|---|---|---|
| seq | one agent at a time, one process; the reference | `seq` (default) |
| mpi | MPI ranks over RMA rings (`backends/mpi/mpi-rma`) | `mpi` |

The previous surface and its backends (including the CUDA backend, its device heap, warp lowering
and `cargo nv`) are on the `legacy` branch, for reference only.
