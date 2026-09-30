# trame

One contract, several machines. [backend.md](backend.md) says what every backend must do and names
none of them. Each backend says what it does, and where it falls short, in its own `README.md`:

| backend | runs on | feature |
|---|---|---|
| [none](backends/none/README.md) | one process, no transport | default |
| [mpi](backends/mpi/README.md) | MPI ranks: RMA-ring lanes, MPI-message batches | `mpi`, `lossy` |
| [nv](backends/nv/README.md) | one CUDA warp per rank | `nv`, `cuda` |

`backends/host/` is the host lowering none and mpi share. `optim/` holds portable primitives
(`exp`); a backend's `optim` re-exports them and may shadow any with its own.

## Conformance

`bash trame/scripts/conform.sh local` runs `conformance/claims.rs` on none, nv's host model, mpi
and lossy, adds X1 (`cargo test`), X2 (declaration fixtures) and P1 (public surface), and prints a
claim × backend table. It exits 0 only when every cell passes. [execution.md](execution.md) keeps
the evidence of each run.
