"""The scaling benchmarks of scale/ and their plain-MPI baselines: one job, one bench, one implementation.

Parameters: bench (barrier, ring64, ring4096, idle, fanin, graph64, graph1024, graph1024s), impl (trame or
mpi), nodes, fill (half or full: workers per node, from the physical cores), the sizes (rounds, steps, msgs,
seconds, cap), and for the graph graph_iters, graph_steps, degree, seed. Also timeout (default 900) and the
mpirun parameters of mpirun.py. Environment: BIN, NIX_LAB_DIR.

trame: W = nodes * per_node workers and the leader as the last rank (W+1 ranks).
mpi: the C baseline with W ranks and no leader. idle and fanin have no baseline: that job records nothing.

Each rank runs `time` (GNU time) around its program. Its stderr starts with its host and ends with the time
line; mpirun's --output tag prefixes each line with [job,rank]<stream>:, which matches a line to its rank.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

from nixsci import lab

import mpirun
import proc

FAMILY = {"barrier": "barrier", "ring64": "ring", "ring4096": "ring", "idle": "idle", "fanin": "fanin",
          "graph64": "graph", "graph1024": "graph", "graph1024s": "graph"}
RING_SIZE = {"ring64": 64, "ring4096": 4096}
GRAPH_SIZE = {"graph64": 64, "graph1024": 1024, "graph1024s": 1024}
RANK_LINE = re.compile(r"^\[\d+,(\d+)\]<(stdout|stderr)>: ?(.*)$")  # mpirun 5 writes a space after the colon
RANK_WRAP = 'echo "HOST $(uname -n)" 1>&2; exec time -f "CPU %U %S %e" "$@"'


def per_node_of(fill: str) -> int:
    """Workers per node. Physical cores are the unique (SOCKET, CORE) pairs of lscpu; an unknown fill is a KeyError."""
    out = proc.run(["lscpu", "-p=SOCKET,CORE"], 30.0).stdout
    cores = len({tuple(line.split(",")) for line in out.splitlines() if line and line[0] != "#"})
    return {"full": cores - 2, "half": max(3, cores // 2)}[fill]


def program(bench: str, impl: str, workers: int, p: dict, bindir: Path) -> list[str] | None:
    """The program and arguments of one rank's job, or None when the implementation has no baseline."""
    # Strong scaling: ITERS // nodes, so ITERS x workers is fixed for one fill.
    iters = int(p.get("graph_iters", 100000)) // (int(p.get("nodes", 1)) if bench == "graph1024s" else 1)
    graph = [workers, p.get("graph_steps", 2000), GRAPH_SIZE.get(bench, 0), p.get("degree", 8), iters, p.get("seed", 1)]
    if impl == "trame":
        table = {
            "barrier": ["scale_barrier", workers, p.get("rounds", 10000)],
            "ring": ["scale_ring", workers, p.get("steps", 10000), RING_SIZE.get(bench, 0), p.get("cap", 8)],
            "idle": ["scale_idle", workers, p.get("seconds", 3)],
            "fanin": ["scale_fanin", workers, p.get("msgs", 10000)],
            "graph": ["scale_graph", *graph],
        }
    else:
        table = {
            "barrier": ["mpi_barrier", workers, p.get("rounds", 10000)],
            "ring": ["mpi_ring", workers, p.get("steps", 10000), RING_SIZE.get(bench, 0)],
            "graph": ["mpi_graph", *graph],
        }
    family = FAMILY[bench]
    # The graph needs W > DEGREE distinct offsets (scale_graph refuses otherwise): such a job records nothing.
    if family not in table or (family == "graph" and workers <= int(p.get("degree", 8))):
        return None
    binary, *args = table[family]
    return [str(bindir / binary)] + [str(a) for a in args]


def parse(stdout: str, stderr: str) -> tuple[list[dict], dict[int, dict]]:
    """(JSON lines of the leader, the last rank, or of an mpi baseline's rank 0; {rank: times} of all ranks)."""
    lines: list[dict] = []
    ranks: dict[int, dict] = {}
    for text in stdout.splitlines():
        m = RANK_LINE.match(text)
        if m and m.group(2) == "stdout" and m.group(3).startswith("{"):
            obj = json.loads(m.group(3))
            if "bench" in obj:
                lines.append(obj)
    for text in stderr.splitlines():
        m = RANK_LINE.match(text)
        if not m or m.group(2) != "stderr":
            continue
        rank, body = int(m.group(1)), m.group(3)
        if body.startswith("HOST "):
            ranks.setdefault(rank, {})["host"] = body[5:].strip()
        elif body.startswith("CPU "):
            user, system, wall = body[4:].split()
            ranks.setdefault(rank, {}).update(user_s=float(user), sys_s=float(system), wall_s=float(wall))
    return lines, ranks


def record(bench: str, impl: str, nodes: int, per_node: int, workers: int, lines: list[dict],
           ranks: dict[int, dict]) -> None:
    """One row per worker line and per rank: the datasets of the README, with the common columns."""
    common = {"bench": FAMILY[bench], "impl": impl, "nodes": nodes, "workers": workers, "per_node": per_node}
    for obj in lines:
        if obj.get("workers") != workers:
            raise SystemExit(f"{obj['bench']}: workers {obj.get('workers')} but the job has {workers}")
        worker = int(obj.get("worker", -1))
        if FAMILY[bench] == "barrier":
            row = {"worker": worker, "rounds": obj["rounds"], "ns": obj["ns"]}
        elif FAMILY[bench] == "ring":
            row = {"worker": worker, "steps": obj["steps"], "size": obj["size"], "cap": obj.get("cap"), "ns": obj["ns"]}
        elif FAMILY[bench] == "idle":
            row = {"worker": worker, "seconds": obj["seconds"], "ns": obj["ns"]}
        elif FAMILY[bench] == "graph":
            row = {"worker": worker, "steps": obj["steps"], "size": obj["size"], "degree": obj["degree"],
                   "iters": obj["iters"], "ns": obj["ns"], "compute_ns": obj["compute_ns"], "checksum": str(obj["checksum"])}
        else:
            row = {"worker": worker, "msgs": obj["msgs"], "ns": obj["ns"]}
        lab.record(FAMILY[bench], {**common, **row})
    for rank in sorted(ranks):
        r = ranks[rank]
        lab.record("cpu", {**common, "rank": rank, "host": r["host"], "user_s": r["user_s"],
                           "sys_s": r["sys_s"], "wall_s": r["wall_s"]})


def main() -> None:
    p = lab.params()
    bench, impl = p["bench"], p["impl"]
    if bench not in FAMILY or impl not in ("trame", "mpi"):
        raise SystemExit(f"unknown bench {bench!r} or impl {impl!r}")
    nodes = int(p.get("nodes", 1))
    per_node = per_node_of(p["fill"])
    workers = nodes * per_node
    bindir = Path(os.environ["BIN"])
    args = program(bench, impl, workers, p, bindir)
    if args is None:
        return print(f"{bench} {impl}: no job (no baseline, or W <= degree): nothing measured", file=sys.stderr)
    mpirun.record_config()
    ranks = workers + 1 if impl == "trame" else workers
    cmd = mpirun.argv(ranks, {**p, "leader": impl == "trame"}) + ["--output", "tag", "sh", "-c", RANK_WRAP, "sh"] + args
    mpirun.record_argv(cmd)
    res = proc.run(cmd, float(p.get("timeout", 900)))
    stem = f"{bench}-{impl}-{nodes}-{per_node}"
    logs = Path(os.environ["NIX_LAB_DIR"]) / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    (logs / f"{stem}.stderr").write_text(res.stderr)
    if res.timed_out or res.rc != 0:
        raise SystemExit(f"{stem}: {'timeout' if res.timed_out else f'mpirun exit {res.rc}'}; see logs/{stem}.stderr")
    lines, rank_info = parse(res.stdout, res.stderr)
    if len(rank_info) != ranks:
        raise SystemExit(f"{stem}: {len(rank_info)} CPU lines for {ranks} ranks")
    record(bench, impl, nodes, per_node, workers, lines, rank_info)


if __name__ == "__main__":
    main()
