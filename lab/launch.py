"""The launch of a backend, as the lab runners use it: the only way a runner starts a trame program.

A backend's `launch` (src/backends/<name>/launch; the lab's environment has its path in LAUNCH) runs a program built for
that backend as H trame hosts, H = the job's machines x split, and writes what each host did into a directory:
host<h>.out, host<h>.err, cpu.tsv, config.txt. It exits 124 when its --timeout ended the run and 2 when it refuses
the job (a split the backend does not have, or a usage error). Nothing here knows how a backend starts its hosts.

Parameters read from lab.params(): split (hosts per machine, default 1), bind (default "none"), timeout (seconds)."""

from __future__ import annotations

import os
import shlex
import tempfile
from dataclasses import dataclass
from pathlib import Path

from nixsci import lab
from nixsci.lab import proc

GUARD = 60  # seconds the runner waits beyond the launch's own timeout before it kills the launch
_recorded = False


@dataclass(frozen=True)
class Run:
    rc: int  # the launch's exit status; 124 when a timeout ended it
    hosts: list[str]  # each host's stdout
    errs: list[str]  # each host's stderr
    cpu: list[dict]  # one row per rank: host, rank, machine, user_s, sys_s, wall_s
    config: str  # what the launch says it ran under (config.txt)
    launch_err: str  # the launch's own stderr
    seconds: float  # the longest wall clock of a rank (GNU time); the launch's own when there is none

    @property
    def timed_out(self) -> bool:
        return self.rc == 124

    @property
    def refused(self) -> bool:
        return self.rc == 2 and not self.hosts

    @property
    def stderr(self) -> str:
        return "".join(self.errs) + self.launch_err


def machines() -> int:
    """The machines of this job: the lines of NIX_DEPLOY_HOSTFILE, or one."""
    path = os.environ.get("NIX_DEPLOY_HOSTFILE")
    n = len([x for x in Path(path).read_text().splitlines() if x.strip()]) if path else 0
    return max(1, n)


def hosts_of_job(params: dict) -> int:
    """The trame hosts of the job: machines x split."""
    return machines() * int(params.get("split", 1))


def _number(text: str) -> float | None:
    try:
        return float(text)
    except ValueError:
        return None


def run(argv: list[str], params: dict, timeout: float) -> Run:
    """Run `argv` (a program and its arguments) through the launch; read back what the hosts wrote."""
    launch = os.environ["LAUNCH"]
    out = Path(tempfile.mkdtemp(prefix="launch-"))
    cmd = [launch, "--split", str(int(params.get("split", 1))), "--bind", str(params.get("bind", "none")),
           "--timeout", str(timeout), "--out", str(out), "--", *argv]
    res = proc.run(cmd, timeout + GUARD)
    rc = 124 if res.rc is None else res.rc
    names = sorted(out.glob("host*.out"), key=lambda p: int(p.stem[4:]))
    hosts = [p.read_text(errors="replace") for p in names]
    errs = [p.with_suffix(".err").read_text(errors="replace") if p.with_suffix(".err").exists() else "" for p in names]
    cpu = []
    table = out / "cpu.tsv"
    if table.exists():
        rows = [line.split("\t") for line in table.read_text().splitlines() if line]
        for row in rows[1:]:
            h, rank, machine, user, system, wall = (row + [""] * 6)[:6]
            cpu.append({"host": int(h), "rank": int(rank), "machine": machine, "user_s": _number(user) or 0.0,
                        "sys_s": _number(system) or 0.0, "wall_s": _number(wall) or 0.0})
    config = (out / "config.txt").read_text(errors="replace") if (out / "config.txt").exists() else ""
    seconds = max([c["wall_s"] for c in cpu], default=res.seconds)
    return Run(rc, hosts, errs, cpu, config, res.stderr, seconds)


def record_config(r: Run, argv: list[str]) -> None:
    """The first run of a job records what the launch ran under, and the command it was given."""
    global _recorded
    if _recorded:
        return
    _recorded = True
    lab.record("launch_config", {"source": "config", "text": r.config})
    lab.record("launch_config", {"source": "argv", "text": shlex.join(argv)})
