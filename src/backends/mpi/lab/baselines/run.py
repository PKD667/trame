"""The plain-MPI baselines of the lab's benchmarks (experiment `baselines-mpi`, experiments.nix): the C programs of
this directory and the OSU micro-benchmarks, run by mpirun without trame.

Parameters: bench (ping | flow | round | graph), impl (plain: the C programs here; osu: the OSU micro-benchmarks),
per (ranks per machine), size, cap, steps, samples, pattern, rounds, graph_*, bind, timeout (seconds).
Environment: BIN (mpi_barrier, mpi_flow, mpi_graph), OSU (the directory of osu_latency and osu_bw), NIX_LAB_DIR.

The rows have the columns of the trame families of lab/perf.py (lab/columns.json, which the closure copies next to this
file), so an analysis sets a baseline beside the backend it was written for; `backend` is "mpi" and `impl` is plain or osu.
Datasets: the bench's own (flow, round, graph), `osu` (size, value, unit, min, max), `cpu`, and `mpi_config` (mpirun.py)."""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

import mpirun
from nixsci import lab
from nixsci.lab import proc

RANK_LINE = re.compile(r"^\[\d+,(\d+)\]<(stdout|stderr)>: ?(.*)$")  # mpirun 5 writes a space after the colon
RANK_WRAP = 'echo "HOST $(uname -n)" 1>&2; exec time -f "CPU %U %S %e" "$@"'
OSU_ROW = re.compile(r"^\s*(\d+)\s+([\d.]+)(?:\s+([\d.]+)\s+([\d.]+))?")


def g(p: dict, key: str, default):
    return type(default)(p.get(key, default))


def job(p: dict, hosts: int) -> tuple[list[str], int] | None:
    """(program and arguments, ranks per host), or None when the baseline has no such job."""
    bench, impl, per = p["bench"], p["impl"], g(p, "per", 4)
    size, cap = g(p, "size", 64), g(p, "cap", 8)
    if impl == "plain":
        table = {
            "flow": (["mpi_flow", g(p, "pattern", "ring"), per, size, cap, g(p, "steps", 20000)], per),
            # The C baselines of round and graph span the world, not one host: one host only.
            **({"round": (["mpi_barrier", per, g(p, "rounds", 10000)], per),
                "graph": (["mpi_graph", per, g(p, "graph_steps", 2000), size, g(p, "degree", 8), g(p, "graph_iters", 100000),
                           g(p, "seed", 1)], per)} if hosts == 1 else {}),
        }
    elif impl == "osu":
        if hosts > 2:
            return None
        common = ["-m", f"{size}:{size}", "-i", str(g(p, "samples", 20000)), "-x", "100", "-f"]
        table = {"ping": (["osu_latency", *common], 2 if hosts == 1 else 1),
                 "flow": (["osu_bw", *common, "-W", str(cap)], 2 if hosts == 1 else 1)}
    else:
        raise SystemExit(f"unknown impl {impl!r}")
    if bench not in table:
        return None
    argv, per_host = table[bench]
    return [str(a) for a in argv], per_host


def parse(stdout: str, stderr: str) -> tuple[list[tuple[int, str]], dict[int, dict]]:
    """((rank, line) of every stdout line, {rank: host and times} of every rank)."""
    out: list[tuple[int, str]] = []
    ranks: dict[int, dict] = {}
    for text in stdout.splitlines():
        m = RANK_LINE.match(text)
        if m and m.group(2) == "stdout":
            out.append((int(m.group(1)), m.group(3)))
    for text in stderr.splitlines():
        m = RANK_LINE.match(text)
        if not m or m.group(2) != "stderr":
            continue
        rank, body = int(m.group(1)), m.group(3)
        if body.startswith("HOST "):
            ranks.setdefault(rank, {})["machine"] = body[5:].strip()
        elif body.startswith("CPU "):
            user, system, wall = body[4:].split()
            ranks.setdefault(rank, {}).update(user_s=float(user), sys_s=float(system), wall_s=float(wall))
    return out, ranks


COLUMNS = json.loads((Path(__file__).parent / "columns.json").read_text())  # lab/columns.json, copied next to this file


def row_of(dataset: str, common: dict, obj: dict) -> dict:
    """A row with exactly the columns of the dataset (lab/columns.json): the program's line over the job's columns."""
    fields = {k: v for k, v in obj.items() if k not in ("bench", "hosts")}
    if "checksum" in fields:  # a u64: text, a column cannot hold it
        fields["checksum"] = str(fields["checksum"])
    unknown = set(fields) - set(COLUMNS[dataset]) - set(common)
    if unknown:
        raise SystemExit(f"{dataset}: the program printed {sorted(unknown)}, which lab/columns.json does not have")
    row = {**common, **fields}
    return {c: row.get(c) for c in (*COLUMNS["common"], *COLUMNS[dataset])}


def record(p: dict, hosts: int, per_host: int, lines: list[tuple[int, str]], ranks: dict[int, dict]) -> int:
    """The rows of one job; answers how many benchmark rows it recorded."""
    bench, impl = p["bench"], p["impl"]
    common = {"bench": bench, "impl": impl, "backend": "mpi", "machines": hosts, "hosts": hosts, "per_host": per_host}
    rows = 0
    for rank, text in lines:
        if impl == "osu":
            m = OSU_ROW.match(text)
            if not m:
                continue
            lab.record("osu", row_of("osu", common, {
                "size": int(m.group(1)), "value": float(m.group(2)), "unit": "us" if bench == "ping" else "MB/s",
                "min": float(m.group(3)) if m.group(3) else None, "max": float(m.group(4)) if m.group(4) else None}))
            rows += 1
            continue
        if not text.startswith("{"):
            continue
        obj = json.loads(text)
        if obj.get("bench") not in (bench, f"mpi_{bench}", "mpi_barrier" if bench == "round" else None):
            continue
        lab.record(bench, row_of(bench, common, obj))
        rows += 1
    for rank in sorted(ranks):
        r = ranks[rank]
        lab.record("cpu", row_of("cpu", common, {
            "host": rank // per_host, "rank": rank, "machine": r.get("machine", ""),
            "user_s": r.get("user_s", 0.0), "sys_s": r.get("sys_s", 0.0), "wall_s": r.get("wall_s", 0.0)}))
    return rows


def main() -> None:
    p = lab.params()
    hosts = len(mpirun.hosts()) or 1
    spec = job(p, hosts)
    if spec is None:
        return print(f"{p['bench']} {p['impl']} on {hosts} hosts: no such job, nothing measured", file=sys.stderr)
    argv, per_host = spec
    root = Path(os.environ["BIN"] if p["impl"] == "plain" else os.environ["OSU"])
    argv[0] = str(root / argv[0])
    mpirun.record_config()
    ranks = hosts * per_host
    cmd = mpirun.argv(ranks, {**p, "per_node": per_host, "leader": False}) + ["--output", "tag", "sh", "-c", RANK_WRAP, "sh"] + argv
    mpirun.record_argv(cmd)
    res = proc.run(cmd, float(p.get("timeout", 600)))
    stem = "-".join(str(p.get(k, "")) for k in ("bench", "impl", "pattern", "size", "cap", "per")) + f"-h{hosts}"
    logs = Path(os.environ["NIX_LAB_DIR"]) / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    (logs / f"{stem}.stderr").write_text(res.stderr)
    (logs / f"{stem}.stdout").write_text(res.stdout)
    if res.timed_out or res.rc != 0:
        raise SystemExit(f"{stem}: {'timeout' if res.timed_out else f'mpirun exit {res.rc}'}; see logs/{stem}.stderr")
    lines, rank_info = parse(res.stdout, res.stderr)
    if len(rank_info) != ranks:
        raise SystemExit(f"{stem}: {len(rank_info)} CPU lines for {ranks} ranks")
    if record(p, hosts, per_host, lines, rank_info) == 0:
        raise SystemExit(f"{stem}: no benchmark rows")


if __name__ == "__main__":
    main()
