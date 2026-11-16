# transport

The checks of `src/backends/mpi/tests/run.sh` (fifo, fair, deadlock, publication) on one host, through `mpirun`. One local run:
`transport-mpi`. Not in the suite. A split or several machines would give each host too few ranks; routes between hosts are
checked by the programs (`lab/programs`) and by `xhost`.

Parameters: `repeats` (1), `timeout` (120 s), and the `mpirun` parameters of `../lib/mpirun.py`.

Dataset `check`, key `check`, `workers`, `case_seed`, `repeat`: `check` str, `workers` int (the fifo checks start workers+1
ranks, the leader last), `case_seed` int (0 for a check with no `--seed`), `repeat` int, `ran` bool, `ok` bool, `seconds`
float (noisy). Also `mpi_config`. Logs go to `$NIX_LAB_DIR/logs`.

## Claims

None yet.

## Planned (no analysis yet)

- Every row has `ok = true` (exact).

## Figures

- `transport-checks`: per run name and check, the workers, the rows, how many are `ok` and the median seconds. A table: the
  checks have a verdict, not a quantity.

## Run

    nix run .#transport-mpi
    nix build .#draft.transport
