"""The MPI transport checks of backends/mpi/tests/run.sh, as run.sh runs them.

run.sh has two parts. Its first part runs the checks on one world (no node split). Its `multi k`
part runs a shorter list with TRAME_MPI_NODE_RANKS = k (nodes of k ranks on this machine). So the
sweep of node_ranks in flake.nix (0, 1, 2) is run.sh's whole set: node_ranks 0 is the first part,
node_ranks 1 and 2 are `multi 1` and `multi 2`. mpirun.argv sets TRAME_MPI_NODE_RANKS from node_ranks.

Parameters: repeats (default 1), timeout (seconds, default 120), and the mpirun parameters of
mpirun.py. Environment: BIN (the check and latency binaries), NIX_LAB_DIR.

Dataset `check`, one row per check run. `workers` and `case_seed` are the numbers run.sh gives each check:
the fifo checks start workers+1 ranks (the leader is the last); the checks without a --seed (fair,
deadlock, publication) record case_seed 0. A fifo-verify row starts no ranks: it reads the directory of its
fifo run. latency is the release binary `latency`. Logs go to $NIX_LAB_DIR/logs.
"""

from __future__ import annotations

import os
import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path

from nixsci import lab

import mpirun
from nixsci.lab import proc

LATENCY = ["--samples", "6636", "--max-gap-ns", "500000", "--seed", "20261026"]


@dataclass(frozen=True)
class Case:
    name: str
    workers: int
    seed: int
    ranks: int | None  # None: run directly, without mpirun
    args: list[str]
    binary: str = "check"


def fifo(root: Path, workers: int, seed: int) -> list[Case]:
    d = root / f"fifo-{workers}-{seed}"
    cases = [Case("fifo", workers, seed, workers + 1, ["fifo", "--seed", str(seed), "--dir", str(d)])]
    # verify reads one log per rank from one directory and compares timestamps across ranks. Over several hosts
    # that is wrong twice: each rank writes its log on its own host, and the timestamps come from different
    # machines' CLOCK_MONOTONIC (spec 5.1: each node has its own clock), so "C was read before the consume began"
    # compares two clocks. It is not run there. The fifo run itself still checks, at the receiver and without a
    # clock, that every route delivers its frames in order, once, with their content. What stays unchecked over
    # several hosts is the credit bound (Full exactly at cap), which needs a common clock.
    if len(mpirun.hosts()) <= 1:
        cases.append(Case("fifo-verify", workers, seed, None, ["verify", str(d)]))
    return cases


def first_part(root: Path) -> list[Case]:
    """run.sh's first part: the fifo runs of W = 3, 4, 6 with seeds 1-5, then the other checks."""
    cases = [c for w in (3, 4, 6) for s in range(1, 6) for c in fifo(root, w, s)]
    cases += [
        Case("overwrite", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000"]),
        Case("overwrite-late", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000", "--late"]),
        Case("fair", 4, 0, 4, ["fair", "--msgs", "16"]),
        Case("barrier", 6, 3, 6, ["barrier", "--seed", "3", "--rounds", "300"]),
        Case("deadlock", 5, 0, 5, ["deadlock"]),
        Case("publication", 3, 0, 3, ["publication"]),
        Case("latency", 3, 20261026, 3, LATENCY + ["--dir", str(root / "latency")], binary="latency"),
    ]
    return cases


def multi_part(root: Path) -> list[Case]:
    """run.sh's `multi k` part (the same for every k): fifo seeds 1, 2 with W = 3, then the other checks."""
    cases = [c for s in (1, 2) for c in fifo(root, 3, s)]
    cases += [
        Case("overwrite", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000"]),
        Case("overwrite-late", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000", "--late"]),
        Case("fair", 4, 0, 4, ["fair", "--msgs", "16"]),
        Case("barrier", 6, 3, 6, ["barrier", "--seed", "3", "--rounds", "300"]),
        Case("deadlock", 5, 0, 5, ["deadlock"]),
        Case("publication", 3, 0, 3, ["publication"]),
        Case("latency", 3, 20261026, 3, LATENCY + ["--dir", str(root / "latency")], binary="latency"),
    ]
    return cases


def plan(root: Path, node_ranks: int) -> list[Case]:
    """The checks of run.sh for one node_ranks value: 0 is the first part, k > 0 is `multi k`."""
    return first_part(root) if node_ranks == 0 else multi_part(root)


def main() -> None:
    params = lab.params()
    repeats = int(params.get("repeats", 1))
    timeout = float(params.get("timeout", 120))
    node_ranks = int(params.get("node_ranks", 0))
    mpirun.record_config()
    logs = Path(os.environ["NIX_LAB_DIR"]) / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="transport-"))
    failed = 0
    try:
        for repeat in range(1, repeats + 1):
            for case in plan(tmp / f"r{repeat}", node_ranks):
                cmd = [str(Path(os.environ["BIN"]) / case.binary)] + case.args
                if case.ranks is not None:
                    cmd = mpirun.argv(case.ranks, params) + cmd
                res = proc.run(cmd, timeout)
                ok = res.rc == 0
                failed += not ok
                (logs / f"{case.name}-{case.workers}-{case.seed}-{repeat}.log").write_text(res.stdout + res.stderr)
                lab.record("check", {
                    "check": case.name,
                    "workers": case.workers,
                    "case_seed": case.seed,
                    "repeat": repeat,
                    "ok": ok,
                    "seconds": float(res.seconds),
                })
                state = "ok" if ok else ("timeout" if res.timed_out else "fail")
                print(f"{state:7} {case.name} {case.workers} {case.seed} {repeat} {res.seconds:.3f}s", flush=True)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print(f"checks not ok: {failed}")


if __name__ == "__main__":
    main()
