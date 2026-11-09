"""The MPI transport checks of src/backends/mpi/tests/run.sh, as run.sh runs them.

The checks run on one host: a split or several machines would make several hosts, each with too few
ranks for the check. Routes between hosts are checked by the programs (programs.py, xhost_ring, peers).

Parameters: repeats (default 1), timeout (seconds, default 120), and the mpirun parameters of
mpirun.py. Environment: BIN (the check binary), NIX_LAB_DIR.

Dataset `check`, one row per check run. `workers` and `case_seed` are the numbers run.sh gives each check:
the fifo checks start workers+1 ranks (the leader is the last); the checks without a --seed (fair,
deadlock, publication) record case_seed 0. A fifo-verify row starts no ranks: it reads the directory of its
fifo run. Logs go to $NIX_LAB_DIR/logs.
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


def plan(root: Path) -> list[Case]:
    """src/backends/mpi/tests/run.sh: the fifo runs of W = 3, 4, 6 with seeds 1-5, then the other checks. The
    transport checks run on one host; routes between hosts are checked by programs (xhost_ring, peers)."""
    cases = [c for w in (3, 4, 6) for s in range(1, 6) for c in fifo(root, w, s)]
    cases += [
        Case("overwrite", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000"]),
        Case("overwrite-late", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000", "--late"]),
        Case("fair", 4, 0, 4, ["fair", "--msgs", "16"]),
        Case("bigroute", 3, 0, 3, ["bigroute"]),
    ]
    return cases


def main() -> None:
    params = lab.params()
    repeats = int(params.get("repeats", 1))
    timeout = float(params.get("timeout", 120))
    mpirun.record_config()
    logs = Path(os.environ["NIX_LAB_DIR"]) / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="transport-"))
    failed = 0
    try:
        for repeat in range(1, repeats + 1):
            for case in plan(tmp / f"r{repeat}"):
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
                    "ran": ok or bool(res.stdout.strip()),  # a check that ran printed its lines; a dead launcher did not
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
