"""Every program of the table against its .expected output: as programs/run.sh checks them.

Parameters: backend (seq or mpi), repeats (default 1), timeout (seconds, default 120), and the mpirun
parameters of mpirun.py (node_ranks). Environment (set by flake.nix): BIN (program binaries),
EXPECTED (<program>.expected files), RANKS_FILE (the table `<program> <ranks>`).

Dataset `run`, one row per program and repeat. verdict is one of
  ok       exit 0 and stdout equal to the expected text
  diff     exit 0, other stdout
  exit     nonzero exit status
  timeout  the timeout ended it
On mpi, consecutive duplicate lines are collapsed first: refuse prints once per rank (run.sh).
The job exits 0 when every run was recorded, whatever the verdicts: a failed program is a result.
"""

from __future__ import annotations

import hashlib
import os
import sys
from pathlib import Path

from nixsci import lab

import mpirun
import proc


def programs() -> list[tuple[str, int]]:
    rows = []
    for line in Path(os.environ["RANKS_FILE"]).read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            name, ranks = line.split()
            rows.append((name, int(ranks)))
    return rows


def normalise(stdout: str, collapse: bool) -> str:
    """The text that is compared: trailing newlines dropped, one newline per line."""
    text = stdout.rstrip("\n")
    lines = text.split("\n") if text else []
    if collapse:
        lines = [line for i, line in enumerate(lines) if i == 0 or line != lines[i - 1]]
    return "".join(line + "\n" for line in lines)


def command(backend: str, binary: str, ranks: int, params: dict) -> list[str]:
    if backend == "seq":
        return [binary]
    return mpirun.argv(ranks, params) + [binary]


def main() -> None:
    params = lab.params()
    backend = params.get("backend", "seq")
    if backend not in ("seq", "mpi"):
        raise SystemExit(f"backend must be seq or mpi, not {backend!r}")
    repeats = int(params.get("repeats", 1))
    timeout = float(params.get("timeout", 120))
    bindir = Path(os.environ["BIN"])
    expected_dir = Path(os.environ["EXPECTED"])
    stderr_dir = Path(os.environ["NIX_LAB_DIR"]) / "stderr"
    stderr_dir.mkdir(parents=True, exist_ok=True)
    collapse = backend == "mpi"
    if backend == "mpi":
        mpirun.record_config()
    verdicts: dict[str, int] = {}
    for name, ranks in programs():
        expected = (expected_dir / f"{name}.expected").read_text()
        for repeat in range(1, repeats + 1):
            res = proc.run(command(backend, str(bindir / name), ranks, params), timeout)
            if res.stderr:
                (stderr_dir / f"{name}.{repeat}.txt").write_text(res.stderr)
            got = normalise(res.stdout, collapse)
            if res.timed_out:
                verdict = "timeout"
            elif res.rc != 0:
                verdict = "exit"
            else:
                verdict = "ok" if got == expected else "diff"
            verdicts[verdict] = verdicts.get(verdict, 0) + 1
            lab.record("run", {
                "program": name,
                "repeat": repeat,
                "ok": verdict == "ok",
                "verdict": verdict,
                "seconds": float(res.seconds),
                "stdout_sha256": hashlib.sha256(got.encode()).hexdigest(),
            })
            print(f"{verdict:7} {backend} {name} {repeat} {res.seconds:.3f}s", file=sys.stderr)
    print(f"verdicts: {verdicts}", file=sys.stderr)


if __name__ == "__main__":
    main()
