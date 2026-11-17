"""The runner of the benchmark experiments (lab/<experiment>/run.py call `main`): one job, one benchmark, H hosts.

Parameters (lab.params()):
  backend  the backend the programs were built for (a directory of src/backends/); recorded in every row
  bench    the benchmark's name: the dataset it records and the program `bench_<bench>` of lab/bench/
  split    trame hosts per machine (default 1); the job's machines come from the lab
  per, size, cap, steps, samples, gap_ns, pattern, k, n, rounds, seconds, msgs, graph_*: the benchmark's own
  trace    1: bench_flow records and replays every trace (the cost of S9 and its check, at scale)
  bind, timeout: the launch's --bind and the job's limit in seconds
Environment: LAUNCH (the backend's launch), BENCH (the benchmark binaries), COLUMNS (lab/columns.json), NIX_LAB_DIR.

The benchmarks are written against the interface only, so the same job runs on any backend that builds the examples.
It goes through the launch (launch.py), which starts the hosts and writes each one's stdout and each rank's CPU.
Each host is one trame host of PER workers and a leader. Rows are one per worker (or per round trip for ping), with the
job's columns (bench, impl, backend, machines, hosts, per_host), and one per rank in `cpu` (GNU time). No duration is
compared across hosts' clocks.

An experiment's run.py gives `main` two functions:
  argv(p, hosts)          the program (a name in BENCH) and its arguments, or None when there is no such job on this many hosts
  augment(obj, p, hosts)  optional: edits a benchmark line before it is recorded (ping adds its bounds)
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import launch
from nixsci import lab

IMPL = "trame"  # the baselines of a backend record their own (its lab/baselines)


def g(p: dict, key: str, default):
    return type(default)(p.get(key, default))


COLUMNS = json.loads(Path(os.environ["COLUMNS"]).read_text())  # the datasets (lab/columns.json)


def row_of(bench: str, common: dict, obj: dict) -> dict:
    """A row with exactly the columns of the bench's dataset: the benchmark's line over the job's columns."""
    fields = {k: v for k, v in obj.items() if k not in ("bench", "hosts")}
    if "checksum" in fields:  # a u64: text, a column cannot hold it
        fields["checksum"] = str(fields["checksum"])
    row = {**common, **fields}
    unknown = set(fields) - set(COLUMNS[bench]) - set(common)
    if unknown:
        raise SystemExit(f"{bench}: the benchmark printed {sorted(unknown)}, which lab/columns.json does not have")
    return {c: row.get(c) for c in (*COLUMNS["common"], *COLUMNS[bench])}


def record(p: dict, run: launch.Run, augment=None) -> int:
    """The rows of one job; answers how many benchmark rows it recorded."""
    bench, hosts = p["bench"], len(run.hosts)
    common = {"bench": bench, "impl": IMPL, "backend": p["backend"], "machines": launch.machines(), "hosts": hosts,
              "per_host": g(p, "per", 4)}
    rows = 0
    for host, text in enumerate(run.hosts):
        for line in text.splitlines():
            if not line.startswith("{"):
                continue
            obj = json.loads(line)
            if "trace" in obj:
                if not obj["trace"]:
                    raise SystemExit(f"a trace is not a run of the model: {obj['detail']}")
                continue
            if obj.get("bench") != bench:
                continue  # the outcomes line
            if augment:
                augment(obj, p, hosts)
            lab.record(bench, row_of(bench, common, obj))
            rows += 1
    for c in run.cpu:
        lab.record("cpu", {k: c.get(k) for k in COLUMNS["cpu"]} | {k: common[k] for k in COLUMNS["common"]})
    return rows


def main(argv_of, augment=None) -> None:
    """Run the job of the parameters: `argv_of` and `augment` as the module docstring says."""
    p = lab.params()
    hosts = launch.hosts_of_job(p)
    argv = argv_of(p, hosts)
    if argv is None:
        return print(f"{p['bench']} on {hosts} hosts: no such job, nothing measured", file=sys.stderr)
    argv = [str(a) for a in argv]  # the runners write numbers; a process argument is text
    argv[0] = str(Path(os.environ["BENCH"]) / argv[0])
    timeout = float(p.get("timeout", 600))
    res = launch.run(argv, p, timeout)
    launch.record_config(res, argv)
    stem = "-".join(str(p.get(k, "")) for k in ("bench", "pattern", "size", "cap", "per", "k", "gap_ns")) + f"-h{hosts}"
    logs = Path(os.environ["NIX_LAB_DIR"]) / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    (logs / f"{stem}.stderr").write_text(res.stderr)
    (logs / f"{stem}.stdout").write_text("".join(f"# host {h}\n{t}" for h, t in enumerate(res.hosts)))
    if res.refused:
        return print(f"{stem}: the backend refuses {hosts} hosts, nothing measured", file=sys.stderr)
    if res.rc != 0:
        raise SystemExit(f"{stem}: {'timeout' if res.timed_out else f'launch exit {res.rc}'}; see logs/{stem}.stderr")
    if record(p, res, augment) == 0:
        raise SystemExit(f"{stem}: no benchmark rows")
