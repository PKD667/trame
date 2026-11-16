# lab/lib

Modules the runners of the experiments import (on the closure's `PYTHONPATH`).

- `bench.py`: the runner of the benchmark experiments. `main(argv_of, augment=None)` reads the job's parameters, launches the
  bench through `$LAUNCH`, and records the bench's dataset, `cpu` and the config. `lab/columns.json` (`$COLUMNS`) fixes the
  columns every row carries.
- `judge.py`: the verdict on a program's output (`check/programs/run.sh`'s rule: `.expected`, `.panics`, `.h<H>.expected`).
  Used by `programs` and `mutants`.
- `launch.py`: calls a backend's `launch` for a job.

The analyses' helper is `../tables.py`. A backend may keep its own modules in `src/backends/<b>/lab/lib/`.
