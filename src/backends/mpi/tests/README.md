# tests

The transport's own checks, below the interface: N - 1 workers and the leader as rank N - 1, started by
`mpirun -n N`. A failed check panics, and `mpirun` ends the job with a nonzero status.

| path | holds |
|---|---|
| `check.rs`, `check/` | the `check` binary (`backend.nix`: `bins = [ "check" ]`) and its checks |
| `run.sh` | every check, in the debug build; exits non-zero on the first failure |
| `transport.py` | the same checks as a lab experiment |
| `progress/` | the credit-return matrix that chose a shared window over one-sided RMA (`progress/RESULT.md`) |
| `RESULT.md` | what `run.sh` showed, with machine and versions |

Inside `nix develop .#mpi`:

    src/backends/mpi/tests/run.sh
    mpirun --oversubscribe -n 4 target/mpi/debug/check fifo --seed 1 --dir D     # 3 workers
    target/mpi/debug/check verify D
    mpirun --oversubscribe -n 3 target/mpi/debug/check overwrite --seed 7 --cap 4 --msgs 20000 [--late]
    mpirun --oversubscribe -n 4 target/mpi/debug/check fair --msgs 16
    mpirun --oversubscribe -n 3 target/mpi/debug/check bigroute                  # 2 workers

## Not checked

- Several hosts: `run.sh` runs one host. Routes between hosts are checked by the programs (`check/programs/`),
  on splits of one machine, not on a network. Other MPI implementations and other CPUs.
- Proof of the code. Three protocols are model-checked (`check/proofs/`) at SeqCst and without timeouts.
- Starvation under time-varying load. `fair` pre-fills its routes.
