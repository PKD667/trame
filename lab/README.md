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
nix run .#suite               every experiment for every backend (planned, see Suite and report)
nix build .#suite-report      the report derived from the suite's locked runs (planned)
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

A check that depends only on the code, not on concurrency, the network or hardware, is a validate verdict,
not a lab experiment: `lab/validate.nix`, built by `nixsci.lib.validate` (validate/README.md in the
nixsci repository). Each check states its property (INTERFACE.md section), model, subject, checker and scope,
and writes `$out/outcome.json`. A verdict is a derivation. The flake exposes them as
`validate.<system>.verdicts.<name>`, the table as `validate.<system>.table`, and `checks` as the holds.

| check | property | what it runs |
|---|---|---|
| `programs-<b>` | every program prints its `.expected` and exits as its `.panics` says, on backend `b` | the binaries of `b`, through its `launch`, 120 s crash guard |
| `tests-<b>` | every test of the root crate passes on backend `b` in the checked build | each test alone through `b`'s `launch` |
| `rejections-<b>` | every rejection case is judged as its annotations say, with `b` compiled in | the pinned driver over `check/rejections/cases` |
| `mutants-<b>` | each mutant of `b` is killed or spared as its header says | one patch at a time (`src/backends/<b>/mutants`) |

`<b>` stands for a backend's name. The names for each backend are in
`src/backends/README.md` (Lab experiments by backend).

A deterministic backend (`deterministic = true` in its `backend.nix`) may bound its scheduler's steps
(`src/backends/README.md`, Limits); a run over the limit exits 3. It counts steps, not seconds, so a looping mutant is a count and not a timing.

Lab experiments remain for what needs concurrency, a network or hardware: the experiments of the multi-process
backends, the latency experiment and the scale experiments.

Scope numbers are measured, not guessed. A check that runs nothing (no test listed, no case judged) fails.

## Experiments and datasets

**programs** (`programs-<b>`, one per backend and placement, names in `src/backends/README.md`): every program
of `check/programs/` against its `.expected` output. Parameters: `backend`, `split` (trame hosts per machine,
1 or 2; `src/backends/README.md`, Launch), `repeats`.
On H hosts each host runs the whole program and is judged alone, except a program with `<name>.h<H>.expected`,
judged on the hosts' outputs in order (programs.py). Dataset `run`: `program` str, `repeat` int, `ran` bool,
`ok` bool, `verdict` str (`ok`, `diff`, `exit`, `timeout`), `seconds` float, `stdout_sha256` str. Key:
`program`, `repeat`. Equal `stdout_sha256` per program between two backends' experiments on the same machine
is the equivalence of the two backends on that machine.

**transport** (`transport-<b>`, for a backend with its own transport checks): the checks of that backend's
`tests/run.sh` on one host (`transport.py`). Dataset `check`: `check` str, `workers` int, `case_seed` int,
`repeat` int, `ran` bool, `ok` bool, `seconds` float. Key: `check`, `workers`, `case_seed`, `repeat`.

**latency** (`latency`, runner `lab/bench/ping.rs`): round trips between workers, with random gaps between them,
placed on one host (same-host) or on two (cross-host), at several message sizes. Round trips in one run are
autocorrelated, so each run is one sample, and the claims count runs. The bounds are the Q clauses of
`CONTRACT.md`, set per placement and size in `lab/q.nix` for the reference deployment. A bound that is still
null means the experiment measures and no claim is judged on it.

**scale** (`scale-local`, and `scale-n<N>` with `span = N` for N in 1, 2, 4, 8, 16, 32): the
programs of `lab/bench/` and their baselines (`src/backends/README.md`, Benchmarks). One job runs one benchmark
with one implementation at one size. Parameters: `bench`, `impl` (`trame`, or a baseline's name), `nodes`, `fill`,
and the bench's sizes. `fill` is `half` or `full`: workers per node are half the physical cores (at least 3)
or the physical cores less two, where the physical cores are the unique (SOCKET, CORE) pairs of
`lscpu`. Every dataset repeats `bench`, `impl`, `nodes`, `workers` and `per_node` as columns, so claims
can select on them.
- `round`: `worker`, `rounds`, `ns` (impl `trame`: rounds through the leader; a baseline: its barrier calls, `lab/bench/README.md`)
- `ring`: `worker`, `steps`, `size`, `cap`, `ns`
- `idle`: `worker`, `seconds`, `ns`
- `fanin`: `worker` (-1 for the leader's total), `msgs`, `ns`
- `graph`: `worker`, `steps`, `size`, `degree`, `iters`, `ns`, `compute_ns`, `checksum` (text: a u64).
  Benches `graph64` (weak, SIZE 64), `graph1024` (weak, SIZE 1024) and `graph1024s` (strong, SIZE 1024:
  ITERS = graph_iters // nodes, so ITERS x workers is fixed per fill). The dataset's `bench` column is
  `graph` for all three. Select on `size` and `iters`. Parameters `graph_iters` (100000), `graph_steps`
  (2000), `degree` (8), `seed` (1). A job with workers <= degree records nothing.
- `cpu`: `rank`, `host`, `user_s`, `sys_s`, `wall_s`, from GNU time around each rank

Each job also writes a `*_config` dataset, whose name carries the backend (`src/backends/README.md`): the job's
full command line (`argv`, scale only), the load average at its start (`loadavg`), and the settings of the
messaging library in the account.

Timing columns are `noisy`, with `replicates = 3`.

**mutants** (`mutants-<b>` for each multi-process backend, built by `checks.nix`, runner `mutants.py`): conformance
by mutation (`check/mutants/`). Nix builds one build per mutant, its patch applied, and one closure holds them
all with the runner. A deterministic backend's record is the validate verdict `mutants-<b>`, not an
experiment. Parameters: `backend`, `mutant` (a patch id, `none` for the unpatched control), `timeout` (120).
The sweep is every id with a `.patch` or a `.skip` file of `check/mutants/<backend>`, so a mutant that cannot
apply (`m23`, and `m17` on a multi-process backend) is one job that records `how` `skipped`.
Dataset `kill`, one row per program: `mutant`, `rule`, `expect`, `program` (`-` when skipped), `killed`
bool (true for `diff`, `exit` or `timeout`), `how` (`ok`, `diff`, `exit`, `timeout`, `skipped`).
Key: `mutant`, `program`. A multi-process backend also records its `*_config` dataset, as its programs experiment does.
`check/mutants/mutate.sh` is the quick local loop. The lab run is the record.

The rejection suite is a validate verdict (above). The driver fuzzer is not a check: `crates/driver/fuzz/flake.nix`
defines packages, not checks, and `crates/driver/fuzz/run.sh <seed> <batches> <K>` is run by hand.

## Suite and report

The suite runs every experiment for every backend, and the report is derived from the suite's locked runs. Both
are planned, and none of the attribute names below is in the flake yet. `nix run .#suite` runs the suite, and
`nix build .#suite-report` compiles the report.

The report has two parts. The first is the latency validation: for each backend, the `latency` experiment is
judged against the Q clauses of `CONTRACT.md`, as proportion claims judged with `nixsci.stats` (PROOFS.md, Latency).
A backend is accepted only if its latency claims are supported. The second is a side-by-side performance
comparison, with every backend on the same axes: bandwidth against message size, aggregate bandwidth against the
number of hosts, step overhead, and idle CPU.

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
  between 0.8 and 1.2 cores while the run lasts. This is the price of the relay's blocking receive
  (`src/backends/README.md`, Benchmarks).
- **Weak scaling of neighbour exchange.** In `ring` at a fixed `per_node`, the median ns per
  step at N nodes is at most 1.5 times the median at one node, for every N measured.
- **Round growth.** In `round`, the median ns per round grows at most linearly in N.
- **Cost against the baseline.** In `ring`, trame's median is at most 3 times the baseline's at
  the same `nodes`, `per_node` and size.
- **The proxy benchmark.** The four claims of the proxy benchmark's spec (`src/backends/README.md`, Benchmarks)
  on `graph`: overhead at most 1.10 against the baseline, weak-scaling curve within 1.05 of the
  baseline's, relay CPU per frame flat in N, strong-scaling speedup at least 0.9 of the baseline's.

When the analyses exist, they will compute the ratios with their intervals and write them to
`values.json`. The thresholds above are the claims they are judged against. A contradicted claim
will stay in the report.
