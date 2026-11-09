"""The performance families of the lab (lab/README.md): one job, one benchmark, one backend, H hosts.

Parameters (lab.params()):
  backend  the backend the programs were built for (a directory of src/backends/); recorded in every row
  bench    ping | flow | step | round | idle | fanin | graph
  split    trame hosts per machine (default 1); the job's machines come from the lab
  per      workers per host (flow, step, round, idle, fanin, graph); ping has fixed placement
  size, cap, steps, samples, gap_ns, pattern, k, n, rounds, seconds, msgs, graph_*: the benchmark's own
  trace    1: bench_flow records and replays every trace (the cost of S9 and its check, at scale)
  bind, timeout: the launch's --bind and the job's limit in seconds
  deployment  ping only: the key of lab/q.nix whose bounds mark each round trip `over_p50` and `over_p99`
Environment: LAUNCH (the backend's launch), BENCH (the benchmark binaries), BOUNDS (lab/q.nix's `q` as JSON),
NIX_LAB_DIR.

Every benchmark is written against the interface only, so the same job runs on any backend that builds the examples.
It goes through the launch (lab/launch.py), which starts the hosts and writes each one's stdout and each rank's CPU.
Each host is one trame host of PER workers and a leader; a benchmark whose routes cross hosts says so in its header.
Rows are one per worker (or per round trip for ping), with the job's columns (bench, impl, backend, hosts, per_host),
and one per rank in `cpu` (GNU time). No duration is compared across hosts' clocks.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import launch
from nixsci import lab

IMPL = "trame"


def g(p: dict, key: str, default):
    return type(default)(p.get(key, default))


def job(p: dict, hosts: int) -> list[str] | None:
    """The program and its arguments, or None when there is no such job on this many hosts."""
    bench, per = p["bench"], g(p, "per", 4)
    size, cap = g(p, "size", 64), g(p, "cap", 8)
    table = {
        "ping": ["bench_ping", size, g(p, "samples", 20000), g(p, "gap_ns", 0), g(p, "cap", 1)],
        "flow": ["bench_flow", g(p, "pattern", "ring"), per, size, cap, g(p, "steps", 20000)]
                + (["trace"] if g(p, "trace", 0) else []),
        "step": ["bench_step", per, g(p, "k", 1), g(p, "n", 1000000)],
        "round": ["bench_round", per, g(p, "rounds", 10000)],
        "idle": ["bench_idle", per, g(p, "seconds", 3)],
        "fanin": ["bench_fanin", per, g(p, "msgs", 10000)],
        "graph": ["bench_graph", per, g(p, "graph_steps", 2000), size, g(p, "degree", 8), g(p, "graph_iters", 100000),
                  g(p, "seed", 1)],
    }
    if bench not in table or bench == "ping" and hosts > 2 or bench == "graph" and per <= g(p, "degree", 8):
        return None
    return [str(a) for a in table[bench]]


def bounds(p: dict) -> dict:
    """The round-trip bounds of the job's deployment and placement, by size: {"p50": ns|None, "p99": ns|None}."""
    path = os.environ.get("BOUNDS")
    if not path or not p.get("deployment"):
        return {}
    return json.loads(Path(path).read_text()).get(str(p["deployment"]), {})


COLUMNS = json.loads((Path(__file__).parent / "columns.json").read_text())  # the datasets, as lab/perf.nix declares them


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


def record(p: dict, run: launch.Run) -> int:
    """The rows of one job; answers how many benchmark rows it recorded."""
    bench, hosts = p["bench"], len(run.hosts)
    common = {"bench": bench, "impl": IMPL, "backend": p["backend"], "machines": launch.machines(), "hosts": hosts,
              "per_host": g(p, "per", 4)}
    placement = "same-host" if hosts == 1 else "cross-host"
    bound = bounds(p).get(placement, {}).get(str(g(p, "size", 64)), {}) if bench == "ping" else {}
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
            if bench == "ping":
                obj["placement"] = placement
                obj["max_gap_ns"] = g(p, "gap_ns", 0)  # the bound of the drawn gaps; `gap_ns` is this trip's own
                # A round trip is over a bound when it exceeds it. With no bound (null in lab/q.nix) the column is null:
                # not "within", which would let a claim be supported by nothing.
                for key in ("p50", "p99"):
                    obj[f"bound_{key}"] = bound.get(key)
                    obj[f"over_{key}"] = None if bound.get(key) is None else obj["ns"] > bound[key]
            lab.record(bench, row_of(bench, common, obj))
            rows += 1
    for c in run.cpu:
        lab.record("cpu", {k: c.get(k) for k in COLUMNS["cpu"]} | {k: common[k] for k in COLUMNS["common"]})
    return rows


def main() -> None:
    p = lab.params()
    hosts = launch.hosts_of_job(p)
    argv = job(p, hosts)
    if argv is None:
        return print(f"{p['bench']} on {hosts} hosts: no such job, nothing measured", file=sys.stderr)
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
    if record(p, res) == 0:
        raise SystemExit(f"{stem}: no benchmark rows")


if __name__ == "__main__":
    main()
