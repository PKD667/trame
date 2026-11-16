# baselines

Plain-MPI programs of the lab's benchmarks, written against the messaging library and not against trame: `barrier.c`,
`flow.c`, `graph.c` (compiled in the backend's build, `backend.nix`) and the OSU micro-benchmarks of nixpkgs. They measure what
the same communication costs without the runtime. `run.py` runs one under `mpirun` and records rows with the columns of the
trame family (`lab/columns.json`): `backend` is `mpi`, `impl` is `plain` or `osu`.

This directory has no analysis and no figure of its own. Its runs name the experiment they join (`experiment` in
`experiment.nix`), whose analysis sets them beside the backends as series `<backend>/plain` and `<backend>/osu`:

| run | joins | bench | spans |
|---|---|---|---|
| `baseline-flow-bw-mpi-s<N>` | flow | flow.c, shift ring | 1 2 |
| `baseline-flow-agg-mpi-s<N>` | flow | flow.c, size 65536 cap 8 | 1 2 4 |
| `baseline-osu-bw-mpi-s<N>` | flow | osu_bw | 1 2 |
| `baseline-osu-latency-mpi-s<N>` | latency | osu_latency | 1 2 |
| `baseline-round-mpi-s1` | round | barrier.c (not equal work, see `lab/round`) | 1 |
| `baseline-graph-mpi-s1` | graph | graph.c, must print the same checksum per rank | 1 |

Parameters are those of the trame run of the same family. The C ring has no route capacity, so no `cap`, and it does one
barrier before its timed steps, which the trame ring does not: the clocks start differently. Datasets: the bench's own, `osu`
(`size`, `value`, `unit`, `min`, `max`), `cpu`, `launch_config`.

## Claims

None. The claims against a baseline are planned in `lab/flow` and `lab/graph`.

## Figures

None here: the analyses of `flow`, `latency`, `round` and `graph`.

## Run

    nix run .#baseline-flow-bw-mpi-s1
