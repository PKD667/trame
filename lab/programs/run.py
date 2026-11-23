"""Every program of the table against its .expected output: as check/programs/run.sh checks them.

Parameters: backend (a directory of src/backends/), repeats (default 1), timeout (seconds, default 120), split (hosts
per machine, default 1), bind, workers (the workers each host is given, lab/lib/launch.py) and only (comma-separated
programs to run, default all). Environment (set by flake.nix): LAUNCH (the backend's launch), PROGRAMS (the
program binaries), EXPECTED (<program>.expected files) and PANICS (optional: the directory of <program>.panics files,
the programs that must panic, as check/programs/run.sh). The programs are the files of PROGRAMS.

Every run goes through the launch (lab/lib/launch.py). A job of H hosts (machines x split) starts the whole program on
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

The verdicts and their rule are lib/judge.py's; mutants/run.py uses the same.
"""

from __future__ import annotations

import hashlib
import os
import sys
from pathlib import Path

import launch
from judge import expected_for, judge, normalise, panic_line, programs
from nixsci import lab


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
    only = [n for n in str(params.get("only", "")).split(",") if n]
    for name in programs():
        if only and name not in only:
            continue
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



main()
