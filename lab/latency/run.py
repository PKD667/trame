"""Round trips between two workers, with random gaps (lab/lib/bench.py; program bench_ping).

Parameters beyond the benchmark's: deployment, the key of q.nix's bounds. Environment: BOUNDS (q.nix's `q` as JSON).
Each round trip gets the placement, the bound of its drawn gaps, the bounds of the deployment and whether it exceeds them."""

import json
import os
from pathlib import Path

import bench

g = bench.g


def argv(p, hosts):
    if hosts > 2:
        return None
    return ["bench_ping", g(p, "size", 64), g(p, "samples", 20000), g(p, "gap_ns", 0), g(p, "cap", 1)]


def bounds(p, placement):
    """The bounds of the job's deployment and placement at its size: {"p50": ns|None, "p99": ns|None}."""
    path = os.environ.get("BOUNDS")
    if not path or not p.get("deployment"):
        return {}
    return json.loads(Path(path).read_text()).get(str(p["deployment"]), {}).get(placement, {}).get(str(g(p, "size", 64)), {})


def augment(obj, p, hosts):
    placement = "same-host" if hosts == 1 else "cross-host"
    bound = bounds(p, placement)
    obj["placement"] = placement
    obj["max_gap_ns"] = g(p, "gap_ns", 0)  # the bound of the drawn gaps; `gap_ns` is this trip's own
    # Over a bound when it exceeds it. With no bound (null in q.nix) the column is null: not "within", which would
    # let a claim be supported by nothing.
    for key in ("p50", "p99"):
        obj[f"bound_{key}"] = bound.get(key)
        obj[f"over_{key}"] = None if bound.get(key) is None else obj["ns"] > bound[key]


bench.main(argv, augment)
