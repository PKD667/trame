"""The MPI transport checks of backends/mpi/tests, as backends/mpi/tests/run.sh runs them.

Parameters: repeats (default 1), timeout (seconds, default 120), and the mpirun parameters of
mpirun.py (node_ranks). Environment: BIN (the check binary), NIX_LAB_DIR.

Dataset `check`, one row per check run. `workers` and `seed` are the numbers tests/run.sh gives
each check: the fifo checks start workers+1 ranks (the leader is the last); the checks without a
--seed (fair, deadlock) record seed 0. Verify runs (fifo-verify) read the directory of their fifo run
and start no ranks. Logs go to $NIX_LAB_DIR/logs.
"""

from __future__ import annotations

import os
import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path

from nixsci import lab

import mpirun
import proc


@dataclass(frozen=True)
class Case:
    name: str
    workers: int
    seed: int
    ranks: int | None  # None: run directly, without mpirun
    args: list[str]


def plan(root: Path) -> list[Case]:
    """The checks in the order of tests/run.sh."""
    cases: list[Case] = []

    def fifo(workers: int, seed: int) -> None:
        d = root / f"fifo-{workers}-{seed}"
        cases.append(Case("fifo", workers, seed, workers + 1, ["fifo", "--seed", str(seed), "--dir", str(d)]))
        cases.append(Case("fifo-verify", workers, seed, None, ["verify", str(d)]))

    for workers in (3, 4, 6):
        for seed in range(1, 6):
            fifo(workers, seed)
    cases += [
        Case("overwrite", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000"]),
        Case("overwrite-late", 3, 7, 3, ["overwrite", "--seed", "7", "--cap", "4", "--msgs", "20000", "--late"]),
        Case("fair", 4, 0, 4, ["fair", "--msgs", "16"]),
        Case("barrier", 6, 3, 6, ["barrier", "--seed", "3", "--rounds", "300"]),
        Case("deadlock", 5, 0, 5, ["deadlock"]),
    ]
    for seed in range(1, 6):
        fifo(31, seed)
    cases.append(Case("barrier", 31, 3, 32, ["barrier", "--seed", "3", "--rounds", "300"]))
    return cases


def main() -> None:
    params = lab.params()
    repeats = int(params.get("repeats", 1))
    timeout = float(params.get("timeout", 120))
    check = str(Path(os.environ["BIN"]) / "check")
    mpirun.record_config()
    logs = Path(os.environ["NIX_LAB_DIR"]) / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="transport-"))
    failed = 0
    try:
        for repeat in range(1, repeats + 1):
            for case in plan(tmp / f"r{repeat}"):
                cmd = [check] + case.args
                if case.ranks is not None:
                    cmd = mpirun.argv(case.ranks, params) + cmd
                res = proc.run(cmd, timeout)
                ok = res.rc == 0
                failed += not ok
                (logs / f"{case.name}-{case.workers}-{case.seed}-{repeat}.log").write_text(res.stdout + res.stderr)
                lab.record("check", {
                    "check": case.name,
                    "workers": case.workers,
                    "seed": case.seed,
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
