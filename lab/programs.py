"""Every program of the table against its .expected output: as check/programs/run.sh checks them.

Parameters: backend (a directory of src/backends/), repeats (default 1), timeout (seconds, default 120), split (hosts
per machine, default 1) and bind. Environment (set by flake.nix): LAUNCH (the backend's launch), PROGRAMS (the
program binaries), EXPECTED (<program>.expected files) and PANICS (optional: the directory of <program>.panics files,
the programs that must panic, as check/programs/run.sh). The programs are the files of PROGRAMS.

Every run goes through the launch (lab/launch.py). A job of H hosts (machines x split) starts the whole program on
each host, and the launch writes each host's stdout alone. With one host it must equal <name>.expected. With H hosts:
the hosts' outputs one after the other must equal <name>.h<H>.expected when it exists; a program that has an
.h<K>.expected for another K only (it is written for K hosts) is not run; any other program's every host must equal
<name>.expected. A backend that refuses the job (the launch exits 2) records nothing for it.

Dataset `run`, one row per program and repeat. verdict is one of
  ok       exit 0 and stdout equal to the expected text
  diff     exit 0, other stdout
  exit     nonzero exit status (a program with a .panics file: zero status, or no panic line on stderr)
  timeout  the timeout ended it
stdout_sha256 hashes the text compared (host 0's, when each host is compared alone), so it is equal between backends
for the same program. Dataset `launch_config`: what the launch ran under (first run of the job).
The job exits 0 when every run was recorded, whatever the verdicts: a failed program is a result.

mutants.py imports panic_line, read_expected, judge, normalise and expected_for from here: one rule for both runners.
"""

from __future__ import annotations

import hashlib
import os
import sys
from pathlib import Path

import launch
from nixsci import lab


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


def main() -> None:
    params = lab.params()
    backend = params["backend"]
    repeats = int(params.get("repeats", 1))
    timeout = float(params.get("timeout", 120))
    expected_dir = Path(os.environ["EXPECTED"])
    bindir = Path(os.environ["PROGRAMS"])
    stderr_dir = Path(os.environ["NIX_LAB_DIR"]) / "stderr"
    stderr_dir.mkdir(parents=True, exist_ok=True)
    hosts = launch.hosts_of_job(params)
    verdicts: dict[str, int] = {}
    for name in programs():
        panic = panic_line(name)
        how = expected_for(expected_dir, name, hosts, panic)
        if how is None:
            print(f"skip    {backend} {name}: written for another host count than {hosts}", file=sys.stderr)
            continue
        mode, expected = how
        for repeat in range(1, repeats + 1):
            argv = [str(bindir / name)]
            res = launch.run(argv, params, timeout)
            launch.record_config(res, argv)
            if res.refused:
                print(f"skip    {backend} {name}: the backend refuses {hosts} hosts", file=sys.stderr)
                break
            if res.stderr:
                (stderr_dir / f"{name}.{repeat}.txt").write_text(res.stderr)
            each = [normalise(h) for h in res.hosts]
            if mode == "all":
                got = "".join(each)
                verdict = judge(res, got, expected, panic)
            else:
                got = each[0] if each else ""
                verdict = next((v for v in (judge(res, g, expected, panic) for g in each or [""]) if v != "ok"), "ok")
            verdicts[verdict] = verdicts.get(verdict, 0) + 1
            lab.record("run", {
                "program": name,
                "repeat": repeat,
                "ran": verdict in ("ok", "diff"),  # nixsci's health check: every row false = the launch never worked
                "ok": verdict == "ok",
                "verdict": verdict,
                "seconds": float(res.seconds),
                "stdout_sha256": hashlib.sha256(got.encode()).hexdigest(),
            })
            print(f"{verdict:7} {backend} {name} {repeat} {res.seconds:.3f}s", file=sys.stderr)
    print(f"verdicts: {verdicts}", file=sys.stderr)


if __name__ == "__main__":
    main()
