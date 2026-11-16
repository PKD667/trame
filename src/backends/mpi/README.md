# mpi backend

One MPI rank per worker, and the leader on the last rank of each host. A host is a shared-memory node, so a
host of W workers needs W + 1 ranks. The seam it implements is in `src/backends/README.md`; the clauses it
keeps are in `CONTRACT.md`.

The leader is a rank of its own because its calls (`send`, `recv`, `stop`, `join`) come from host code, and
giving that job to one worker would make that worker special. The leader runs no worker code.

| directory | holds | read |
|---|---|---|
| `backend.rs` | `impl Backend for Selected`, a forwarder | (this file) |
| `memory/` | the window's byte layout, the window, the node segment, the abort object | `memory/README.md` |
| `rings/` | send, receive and the questions on the rings of a window | `rings/README.md` |
| `waiting/` | epoch words and futexes: how an agent sleeps and is woken | `waiting/README.md` |
| `transport/` | hosts, nodes, and routes between hosts (lanes) | `transport/README.md` |
| `setup/` | agreement, plan, refusal, window setup, process state, finalize | `setup/README.md` |
| `roles/` | worker, leader, results and aborts, the deadlock detector | `roles/README.md` |
| `tests/` | the transport's own checks, run under `mpirun` | `tests/README.md` |
| `mutants/` | patches against these files, run by the lab | `check/mutants/README.md` |
| `bench/`, `experiments.nix`, `validate.nix` | baselines, lab experiments, the TLC check | `lab/README.md` |
| `mpi-rma/` | a submodule: a crate of its own, not built here | its own README |
| `launch` | the launcher every backend provides | `src/backends/README.md`, Launch |

`mod.rs` holds the module list, `Transport` (one endpoint: a worker's or the leader's) and the items the crate
uses. `types.rs` holds the types the transport hands to callers. `harness.rs` holds what the test binaries
call at the end of a run.

Build and run inside `nix develop .#mpi`, with `--no-default-features --features mpi`; the commands are in
`src/backends/README.md`, Build and run. The protocols are model-checked in `check/proofs/mpi/`
(`check/proofs/README.md`, which cites these files by path).
