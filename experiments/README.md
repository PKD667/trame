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
implementation at one size. Parameters: `bench`, `impl` (`trame` or `mpi`), `nodes`,
`per_node` (workers per node), and the bench's sizes. Every dataset repeats `bench`, `impl`,
`nodes` and `workers` as columns, so claims can select on them.
- `barrier`: `worker`, `rounds`, `ns`
- `ring`: `worker`, `steps`, `size`, `cap`, `ns`
- `idle`: `worker`, `seconds`, `ns`
- `fanin`: `worker` (-1 for the leader's total), `msgs`, `ns`
- `cpu`: `rank`, `host`, `user_s`, `sys_s`, `wall_s`, from GNU time around each rank

Timing columns are `noisy`; `replicates = 3`.

**mutants** (`mutants-seq`, `mutants-mpi`): conformance by mutation (`conformance/`). Nix
builds one closure per mutant with its patch applied; a job runs the program set against one
mutant. Dataset `kill`: `mutant`, `rule`, `program`, `killed` bool, `how` str.

The rejection suite and the driver fuzzer are deterministic compilations, not measurements:
they are Nix checks (`nix flake check`), not experiments.

## Claims, stated before measuring

Analysis `conformance`: every row of `programs-*` and `transport-*` has `ok = true` (exact).
Analysis `mutants`: every mutant whose catalogue entry says `killed` is killed (exact).
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

The ratios are computed by the analysis pipeline, with their intervals, and written to
`values.json`; the thresholds above are the claims they are judged against. A contradicted claim
stays in the report.
