# trame

[backend.md](backend.md) defines the common contract. Each backend's `README.md` records its
implementation and limitations:

| backend | runs on | feature | dependency shell |
|---|---|---|---|
| [none](backends/none/README.md) | one process, no transport | default | `nix develop .#none` |
| [mpi](backends/mpi/README.md) | MPI ranks: RMA-ring lanes, MPI-message batches | `mpi`, `lossy` | `nix develop .#mpi` |
| [nv](backends/nv/README.md) | one CUDA warp per rank | `nv`, `cuda` | `nix develop .#nv` (host model) |

Backend implementations live in `backends/`; Cargo wrappers live in `cargo/<backend>/`.
`backends/host/` is the host lowering none and mpi share. `optim/` holds portable primitives
(`exp`); a backend's `optim` re-exports them and may shadow any with its own. Each backend owns
its Nix dependency export in `backends/<backend>/dependencies.nix`; the root flake composes
backend-specific shells without a shared native-dependency set.

Run the shared verifier as a Cargo subcommand: `cargo install --path verify --locked`, then
`cargo trame check --features mpi`. `cargo-trame` reads Cargo's workspace metadata, checks every
workspace package that depends on trame, and only then delegates to the requested Cargo command.
The backend shells also put this extension on `PATH`. Raw Cargo skips the source preflight; the
`concurrent!` macro still enforces receive ownership during expansion.

## Conformance

`bash scripts/conform.sh local` runs `conformance/claims.rs` on none, nv's host model, mpi
and lossy, adds X1 (`cargo test`), X2 (declaration fixtures), X3 (the shared receive-ownership
verifier) and P1 (public surface), and prints a claim × backend table. It exits 0 only when every
cell passes. [execution.md](execution.md) keeps the evidence of each run.
