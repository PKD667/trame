# lab

Every measurement of trame is a nixsci lab experiment: Nix builds the binaries and the runner into a closure, the lab
executor runs it on hosts, and each run is sealed with its records, its commit and its machine. Analyses are derivations over
the locked runs (`lab.lock`); claims are judged by `nixsci.stats`. No number about trame is quoted unless it comes from a
locked run through an analysis.

## An experiment is a directory

`lab/<name>/` for every backend, `src/backends/<b>/lab/<name>/` for one backend's own. `nix/lab.nix` finds every
`experiment.nix` there; nothing outside `src/backends/<b>/` names a backend.

| file | holds |
|---|---|
| `experiment.nix` | a function `{ pkgs, lib, labLib, system, closure, backends, machines, data, bench, ... }` answering `{ name; figures; runs; claims; claimIds; analysis; }` |
| `run.py` | the runner: reads `lab.params()`, runs the closure's binaries, records typed rows with `lab.record` |
| `analysis.py` | the analysis of this experiment only (`tables.py` is its helper) |
| `README.md` | at most 40 lines: what it measures, parameters, datasets, claims, figures, how to run |

- `runs`: `{ name; kind; backend; span; suite; exp; }`. The name is the app (`nix run .#<name>`) and the key in `lab.lock`.
  `suite = true` puts it in `nix run .#suite`. A run may name another experiment (`experiment`) whose analysis it joins.
- `figures`: the contract. `analysis.py` writes `tables/<figure>.tsv` and `figures/<figure>.tex`; `tables.done()` fails if the
  set written differs from `figures`. Conformance experiments draw a table of verdicts.
- `claims`: nixsci claims as data (kind, dataset, column, bound). `claimIds` are their ids, with `<placeholders>` for what
  varies. The README's `## Claims` lists the same ids as `- \`id\` sentence`. `checks.lab-contract` fails if the two differ, or
  a figure is not named in the README. Claims not yet analysed go under `## Planned (no analysis yet)`.

## Running

    nix run .#<run>                  measure: lease hosts, run the jobs, seal, lock (commit lab.lock)
    nix build .#<experiment>         derive tables, figures and claims from the locked runs; fails naming runs it lacks
    nix build .#draft.<experiment>   the same over the runs that exist (marked draft)
    nix build .#report               every experiment's analysis, one directory each (`.#draft.report` for drafts)
    nixsci lab verify <run>          re-measure one run from the closure it used
    nix run .#suite                  every run with `suite = true`, for every backend, one after another

Analyses are under `legacyPackages`, not `packages`, because a fresh clone may lack the locked runs. A figure source is a
`usi-design` document: `usi-latexmk figures/<figure>.tex` draws it. `lab.lock` is keyed by the closure's inputs: changing a
runner or its binaries makes earlier runs "of older code", which the strict analysis refuses.

## Hosts

A host comes from a provider name. The flake says `local` or `cluster`; what `cluster` means is set in
`~/.config/nixsci/providers.json` on the machine that runs the experiment, never in this repository.

## Verdict or experiment

A check that depends only on the code, not on concurrency, the network or hardware, is a validate verdict
(`nix/validate.nix`), a derivation. The lab holds what needs processes, a network or hardware.

## Experiments

- [programs](programs/README.md), [mutants](mutants/README.md): conformance of every backend.
- [latency](latency/README.md): round trips, same host and cross host, against the bounds of CONTRACT.md.
- [flow](flow/README.md): bandwidth against size and hosts. [step](step/README.md), [idle](idle/README.md).
- [round](round/README.md), [fanin](fanin/README.md), [graph](graph/README.md): scaling of the leader and of the proxy benchmark.
- [simd](simd/README.md): `parallel!` lowered to explicit SIMD.
- [lib](lib/README.md): the runners' shared modules. [bench](bench/README.md): the benchmark programs.
- Multi-process backend: [transport](../src/backends/mpi/lab/transport/README.md), [xhost](../src/backends/mpi/lab/xhost/README.md),
  [baselines](../src/backends/mpi/lab/baselines/README.md).
