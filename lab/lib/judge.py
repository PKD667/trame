"""The rule that judges a program run against its .expected output, as check/programs/run.sh does.

Environment (set by the closure): PROGRAMS (the program binaries), PANICS (optional: the directory of <program>.panics
files, the programs that must panic). A run is judged one of
  ok       exit 0 and stdout equal to the expected text
  diff     exit 0, other stdout
  exit     nonzero exit status (a program with a .panics file: zero status, or no panic line on stderr)
  timeout  the timeout ended it
Used by programs/run.py and mutants/run.py: one rule for both.
"""

from __future__ import annotations

import os
from pathlib import Path

import launch


def programs() -> list[str]:
    """The programs of this build: the executable files of PROGRAMS, by name."""
    return sorted(p.name for p in Path(os.environ["PROGRAMS"]).iterdir() if os.access(p, os.X_OK))


def panic_line(name: str) -> str | None:
    """The line a program must print on stderr when it must panic (<name>.panics), else None."""
    if not os.environ.get("PANICS"):
        return None
    path = Path(os.environ["PANICS"]) / f"{name}.panics"
    return path.read_text().rstrip("\n") if path.exists() else None


def read_expected(directory: Path, name: str, panic: str | None) -> str | None:
    """<name>.expected as text. A program that must panic may have none (leader_panics): None then."""
    path = directory / f"{name}.expected"
    if path.exists():
        return path.read_text()
    if panic is None:
        raise SystemExit(f"{name}: no {name}.expected, and no {name}.panics")
    return None


def normalise(stdout: str) -> str:
    """The text that is compared: trailing newlines dropped, one newline per line."""
    text = stdout.rstrip("\n")
    return text + "\n" if text else ""


def judge(res: launch.Run, got: str, expected: str | None, panic: str | None) -> str:
    """The verdict of one run, as check/programs/run.sh: ok, diff, exit or timeout (the module docstring)."""
    if res.timed_out:
        return "timeout"
    if panic is not None:
        # A program that must panic: non-zero status, and the panic line on stderr. Its stdout is judged
        # only when it has an .expected file.
        if res.rc == 0 or panic not in res.stderr.split("\n"):
            return "exit"
        return "ok" if expected is None or got == expected else "diff"
    if res.rc != 0:
        return "exit"
    return "ok" if got == expected else "diff"


def expected_for(directory: Path, name: str, hosts: int, panic: str | None) -> tuple[str, str | None] | None:
    """How a program is judged on `hosts` hosts: ("each", text) compares every host with text,
    ("all", text) the hosts' outputs one after the other. None: the program is written for another count."""
    if hosts > 1:
        exact = directory / f"{name}.h{hosts}.expected"
        if exact.exists():
            return ("all", exact.read_text())
        if any(directory.glob(f"{name}.h*.expected")):
            return None
    elif any(directory.glob(f"{name}.h*.expected")) and not (directory / f"{name}.expected").exists():
        return None
    return ("each", read_expected(directory, name, panic))
