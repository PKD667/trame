# Experiments

Every script that runs trame's programs is a nixsci lab experiment. Nix builds the binaries
and the runner into a closure, the lab executor runs it on hosts, and each run is sealed with
its records, the commit it came from and the machine. Analyses are Nix derivations over the
locked runs (`lab.lock`), and claims are judged by `nixsci.stats`. No number about trame is
quoted anywhere unless it comes from a locked run through an analysis.

```
nix run .#<experiment>        measure: lease hosts, run the jobs, seal, lock (commit lab.lock)
nix build .#<analysis>        derive: tables, figures and claims from the locked runs only
nixsci lab verify <run>       re-measure one run from the closure it used
```

Hosts come from a provider name. The flake says `local` or `cluster`; what `cluster` means
(which machines, how to reach them) is set in `~/.config/nixsci/providers.json` on the machine
that runs the experiment, never in this repository.

## Runners

One Python runner per experiment, in this directory. It reads its parameters with
`lab.params()`, runs binaries from the closure, compares or parses their output, and records
typed rows with `lab.record`. Programs never know they are measured: they print, and the runner
records.

## Experiments and datasets

**programs** (`programs-seq`, `programs-mpi`; `programs-cluster` with `span = 2`): every
program of `programs/` against its `.expected` output. Parameters: `backend`, `node_ranks` (0 is
the real node layout; k splits one machine into nodes of k ranks), `repeats`.
Dataset `run`: `program` str, `repeat` int, `ok` bool, `verdict` str (`ok`, `diff`, `exit`,
`timeout`), `seconds` float, `stdout_sha256` str. Key: `program`, `repeat`.

**transport** (`transport-mpi`; `transport-cluster` with `span = 2`): the checks of
`backends/mpi/tests`. Dataset `check`: `check` str, `workers` int, `seed` int, `repeat` int,
`ok` bool, `seconds` float. Key: `check`, `workers`, `seed`, `repeat`.

**scale** (`scale-local`; `scale-n<N>` with `span = N` for N in 1, 2, 4, 8, 16, 32): the
programs of `scale/` and their plain-MPI baselines. One job runs one benchmark with one
implementation at one size. Parameters: `bench`, `impl` (`trame` or `mpi`), `nodes`, `fill`, and the
bench's sizes. `fill` is `half` or `full`: workers per node are half the physical cores (at least 3)
or the physical cores less two, where the physical cores are the unique (SOCKET, CORE) pairs of
`lscpu`. Every dataset repeats `bench`, `impl`, `nodes`, `workers` and `per_node` as columns, so claims
can select on them.
- `barrier`: `worker`, `rounds`, `ns`
- `ring`: `worker`, `steps`, `size`, `cap`, `ns`
- `idle`: `worker`, `seconds`, `ns`
- `fanin`: `worker` (-1 for the leader's total), `msgs`, `ns`
- `graph`: `worker`, `steps`, `size`, `degree`, `iters`, `ns`, `compute_ns`, `checksum` (text: a u64).
  Benches `graph64` (weak, SIZE 64), `graph1024` (weak, SIZE 1024) and `graph1024s` (strong, SIZE 1024:
  ITERS = graph_iters // nodes, so ITERS x workers is fixed per fill). The dataset's `bench` column is
  `graph` for all three; select on `size` and `iters`. Parameters `graph_iters` (100000), `graph_steps`
  (2000), `degree` (8), `seed` (1). A job with workers <= degree records nothing.
- `cpu`: `rank`, `host`, `user_s`, `sys_s`, `wall_s`, from GNU time around each rank

Each job also writes `mpi_config` rows (`mpirun.py`): the job's full mpirun command line (`argv`,
scale only), the load average at its start (`loadavg`), and the MCA settings of the account.

Timing columns are `noisy`; `replicates = 3`.

**mutants** (`mutants-seq`, `mutants-mpi`; `checks.nix`, runner `mutants.py`): conformance by mutation
(`conformance/`). Nix builds one backend build per mutant, its patch applied, and one closure holds them
all with the runner. Parameters: `backend`, `mutant` (a patch id, `none` for the unpatched control),
`timeout` (120). The sweep is every id with a `.patch` or a `.skip` file of `conformance/mutants/<backend>`,
so a mutant that cannot apply (`m23`; `m17` on mpi) is one job that records `how` `skipped`.
Dataset `kill`, one row per program: `mutant`, `rule`, `expect`, `program` (`-` when skipped), `killed`
bool (true for `diff`, `exit` or `timeout`), `how` (`ok`, `diff`, `exit`, `timeout`, `skipped`).
Key: `mutant`, `program`. Mpi also records `mpi_config`, as `programs-mpi` does.
`conformance/mutate.sh` remains the quick local loop; the lab run is the record.

The rejection suite and the driver fuzzer are deterministic compilations, not measurements:
they are Nix checks (`nix flake check`), not experiments.

## Claims, stated before measuring

Analysis `conformance`: every row of `programs-*` and `transport-*` has `ok = true` (exact).
Analysis `mutants`: every mutant whose catalogue entry says `killed` and that applies is killed
(exact). A mutant recorded `skipped` (no place to apply it in that backend) is listed as a gap in
the program set, never counted as a kill.
Analysis `scale`, each judged at 0.99 and reported as supported, contradicted or inconclusive:
- **Idle workers sleep.** In `idle`, a rank that hosts no relay uses below 2% of its wall time
  in user plus system time.
- **The relay costs one core.** On more than one node, the rank that hosts the relay uses
  between 0.8 and 1.2 cores while the run lasts. This is the price of MPI's blocking receive.
- **Weak scaling of neighbour exchange.** In `ring` at a fixed `per_node`, the median ns per
  step at N nodes is at most 1.5 times the median at one node, for every N measured.
- **Barrier growth.** In `barrier`, the median ns per round grows at most linearly in N.
- **Cost against plain MPI.** In `ring`, trame's median is at most 3 times the MPI baseline's at
  the same `nodes`, `per_node` and size.
- **The proxy benchmark.** The four claims of `backends/mpi/SCALING.md` (Claims) on `graph`:
  overhead at most 1.10 against the MPI baseline, weak-scaling curve within 1.05 of the
  baseline's, relay CPU per frame flat in N, strong-scaling speedup at least 0.9 of the baseline's.

The ratios are computed by the analysis pipeline, with their intervals, and written to
`values.json`; the thresholds above are the claims they are judged against. A contradicted claim
stays in the report.
