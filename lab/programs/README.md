# programs

Every program of `check/programs/` against its `.expected` output, on every backend, through the backend's `launch`.
On H hosts (machines x split) each host runs the whole program and is judged alone, except a program with
`<name>.h<H>.expected`, judged on the hosts' outputs in order (`lib/judge.py`).

Runs: `programs-<b>` (one local host), `programs-<b>-s<N>` (N machines, N in 1 2 4 within the backend's `maxMachines`).
Parameters: `backend`, `split` (hosts per machine, the backend's `splits`), `repeats` (3). Not run: a program written for
another host count; a job the launch refuses (exit 2) records nothing.

Dataset `run`, key `program`, `repeat`: `program` str, `repeat` int, `ran` bool, `ok` bool, `verdict` str (`ok`, `diff`,
`exit`, `timeout`), `seconds` float, `stdout_sha256` str. Equal `stdout_sha256` per program between two backends on the same
machine is the equivalence of the two backends there. Also `launch_config` (what the launch ran under).

## Claims

- `programs/<backend>/s<span>`: every run of every program on that backend and span is `ok` (exact).

## Figures

- `programs-verdicts`: per run name and program, the runs and their verdicts (`ok`, `diff`, `exit`, `timeout`), the median
  seconds and the number of distinct stdout hashes. A table: a conformance run has no quantity to draw.

## Run

    nix run .#programs-<b>-s2
    nix build .#programs             # strict: every run locked, none stale
    nix build .#draft.programs       # the runs that exist
