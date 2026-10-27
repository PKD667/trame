"""The scaling benchmarks of scale/ and their plain-MPI baselines: one job, one bench, one implementation.

Parameters: bench (barrier, ring64, ring4096, idle or fanin: ring64 and ring4096 are the ring at size
64 and 4096), impl (trame or mpi), nodes, per_node (workers per node), and
the sizes, with defaults rounds 10000, steps 10000, msgs 10000, seconds 3, cap 8.
Also timeout (default 900) and the mpirun parameters of mpirun.py. Environment: BIN, NIX_LAB_DIR.

trame: W = nodes * per_node workers and the leader as the last rank (W+1 ranks).
mpi: the C baseline with W ranks and no leader. idle and fanin have no baseline: that job records nothing.

Each rank runs `time` (GNU time) around its program, so each rank's CPU is measured. The rank's
stderr starts with its host and ends with the time line; mpirun's --output tag prefixes every line with
[job,rank]<stream>:, and that is how a line is matched to its rank.
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

FAMILY = {"barrier": "barrier", "ring64": "ring", "ring4096": "ring", "idle": "idle", "fanin": "fanin"}
RING_SIZE = {"ring64": 64, "ring4096": 4096}
RANK_LINE = re.compile(r"^\[\d+,(\d+)\]<(stdout|stderr)>:(.*)$")
RANK_WRAP = 'echo "HOST $(uname -n)" 1>&2; exec time -f "CPU %U %S %e" "$@"'


def program(bench: str, impl: str, workers: int, p: dict, bindir: Path) -> list[str] | None:
    """The program and arguments of one rank's job, or None when the implementation has no baseline."""
    if impl == "trame":
        table = {
            "barrier": ["scale_barrier", workers, p.get("rounds", 10000)],
            "ring": ["scale_ring", workers, p.get("steps", 10000), RING_SIZE.get(bench, 0), p.get("cap", 8)],
            "idle": ["scale_idle", workers, p.get("seconds", 3)],
            "fanin": ["scale_fanin", workers, p.get("msgs", 10000)],
        }
    else:
        table = {
            "barrier": ["mpi_barrier", workers, p.get("rounds", 10000)],
            "ring": ["mpi_ring", workers, p.get("steps", 10000), RING_SIZE.get(bench, 0)],
        }
    family = FAMILY[bench]
    if family not in table:
        return None
    binary, *args = table[family]
    return [str(bindir / binary)] + [str(a) for a in args]


def parse(stdout: str, stderr: str) -> tuple[list[dict], dict[int, dict]]:
    """(JSON lines of rank 0's stdout, {rank: {host, user_s, sys_s, wall_s}} from the time lines)."""
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


def record(bench: str, impl: str, nodes: int, workers: int, lines: list[dict], ranks: dict[int, dict]) -> None:
    """One row per worker line and per rank: the datasets of the README, with the common columns."""
    common = {"bench": FAMILY[bench], "impl": impl, "nodes": nodes, "workers": workers}
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
    mpirun.record_config()
    nodes, per_node = int(p.get("nodes", 1)), int(p["per_node"])
    workers = nodes * per_node
    leader = impl == "trame"
    bindir = Path(os.environ["BIN"])
    args = program(bench, impl, workers, p, bindir)
    if args is None:
        print(f"no plain-MPI baseline for {bench}: nothing measured", file=sys.stderr)
        return
    ranks = workers + 1 if leader else workers
    cmd = mpirun.argv(ranks, {**p, "leader": leader}) + ["--output", "tag"]
    cmd += ["sh", "-c", RANK_WRAP, "sh"] + args
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
    record(bench, impl, nodes, workers, lines, rank_info)


if __name__ == "__main__":
    main()
