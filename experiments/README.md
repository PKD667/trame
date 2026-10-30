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

Hosts come from a provider name. The flake says `local` or `cluster`. What `cluster` means
(which machines, how to reach them) is set in `~/.config/nixsci/providers.json` on the machine
that runs the experiment, never in this repository.

## Runners

One Python runner per experiment, in this directory. It reads its parameters with
`lab.params()`, runs binaries from the closure, compares or parses their output, and records
typed rows with `lab.record`. Programs never know they are measured: they print, and the runner
records.

## Verdicts (deterministic checks)

A check that depends only on the code, not on threads, the network or hardware, is a validate verdict,
not a lab experiment: `experiments/validate.nix`, built by `nixsci.lib.validate` (validate/README.md in the
nixsci repository). Each check states its property (spec.md section), model, subject, checker and scope,
and writes `$out/outcome.json`. A verdict is a derivation. The flake exposes them as
`validate.<system>.verdicts.<name>`, the table as `validate.<system>.table`, and `checks` as the holds.

| check | property | what it runs |
|---|---|---|
| `programs-seq` | every program prints its `.expected` and exits 0 on seq | the seq binaries, with a 120 s crash guard |
| `seq-tests` | the root crate's seq tests pass in the checked build | `cargo test`, the exhaustive test excluded |
| `scheduler-exhaustive` | start and end agree with the spec rule on all states up to 2 processes, 3 cells | `cargo test --release` of that one test |
| `rejections` | every rejection case is judged as its expectation says | `rejections/run.sh` (driver pinned) |
| `mutants-seq` | each seq mutant is killed or spared as its header says | one patch at a time, step limit (`conformance/README.md`) |

`TRAME_SEQ_STEP_LIMIT` (`backends/seq/limit.rs`) bounds the seq scheduler's steps, and a run over the limit
exits 3. It counts steps, not seconds, so a looping mutant is a count and not a timing.

Lab experiments remain for what needs threads, a network or hardware: `programs-mpi`, `transport-mpi`,
`mutants-mpi` and the scale experiments.

Scope numbers are measured, not guessed: `scheduler-exhaustive` states and end checks, `seq-tests`
count. A change to either fails the check until its scope is updated.

## Experiments and datasets

**programs** (`programs-mpi`, and `programs-cluster` with `span = 2`): every
program of `programs/` against its `.expected` output on mpi. The seq run is a verdict (below). Parameters: `backend`, `node_ranks` (0 is
the real node layout, k splits one machine into nodes of k ranks), `repeats`.
Dataset `run`: `program` str, `repeat` int, `ok` bool, `verdict` str (`ok`, `diff`, `exit`,
`timeout`), `seconds` float, `stdout_sha256` str. Key: `program`, `repeat`.

**transport** (`transport-mpi`, and `transport-cluster` with `span = 2`): the checks of
`backends/mpi/tests/run.sh` (`transport.py`). The sweep of `node_ranks` (0, 1, 2) is run.sh's whole set:
0 is its first part, 1 and 2 are its `multi 1` and `multi 2` parts. Dataset `check`: `check` str, `workers`
int, `case_seed` int, `repeat` int, `ok` bool, `seconds` float. Key: `check`, `workers`, `case_seed`, `repeat`. The
dataset has no `node_ranks` column, so rows of different splits share a key: `flake.nix` `transportData`
needs that column before an analysis merges them (not changed here).

**scale** (`scale-local`, and `scale-n<N>` with `span = N` for N in 1, 2, 4, 8, 16, 32): the
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
  `graph` for all three. Select on `size` and `iters`. Parameters `graph_iters` (100000), `graph_steps`
  (2000), `degree` (8), `seed` (1). A job with workers <= degree records nothing.
- `cpu`: `rank`, `host`, `user_s`, `sys_s`, `wall_s`, from GNU time around each rank

Each job also writes `mpi_config` rows (`mpirun.py`): the job's full mpirun command line (`argv`,
scale only), the load average at its start (`loadavg`), and the MCA settings of the account.

Timing columns are `noisy`, with `replicates = 3`.

**mutants** (`mutants-mpi`, built by `checks.nix`, runner `mutants.py`): conformance by mutation (`conformance/`).
Nix builds one mpi build per mutant, its patch applied, and one closure holds them all with the runner.
The seq record is the validate verdict `mutants-seq`, not an experiment. Parameters: `backend`, `mutant` (a patch id, `none` for the unpatched control),
`timeout` (120). The sweep is every id with a `.patch` or a `.skip` file of `conformance/mutants/<backend>`,
so a mutant that cannot apply (`m23`, and `m17` on mpi) is one job that records `how` `skipped`.
Dataset `kill`, one row per program: `mutant`, `rule`, `expect`, `program` (`-` when skipped), `killed`
bool (true for `diff`, `exit` or `timeout`), `how` (`ok`, `diff`, `exit`, `timeout`, `skipped`).
Key: `mutant`, `program`. Mpi also records `mpi_config`, as `programs-mpi` does.
`conformance/mutate.sh` is the quick local loop. The lab run is the record.

The rejection suite is a validate verdict (above). The driver fuzzer is not a check: `driver/fuzz/flake.nix`
defines packages, not checks, and `driver/fuzz/run.sh <seed> <batches> <K>` is run by hand.

## Claims, stated before measuring

No analysis named here is in the tree yet: there is no `conformance`, `mutants` or `scale` analysis, and no
`values.json`. The list below is the plan: nothing in the tree supports or contradicts these claims yet.

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

When the analyses exist, they will compute the ratios with their intervals and write them to
`values.json`. The thresholds above are the claims they are judged against. A contradicted claim
will stay in the report.
