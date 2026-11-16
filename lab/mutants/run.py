"""Conformance by mutation: one mutant against every program of the table (check/mutants/README.md).

Parameters: backend (a directory of src/backends/), mutant (a patch id such as m01, or none: the unpatched build,
the control), timeout (seconds, default 120), and split and bind as lib/launch.py reads them.
Environment (set by experiment.nix): LAUNCH (the backend's launch), MUTANTS (one directory per id, with programs/ and
share/expected/ of that mutant's build), PATCHES (src/backends/<backend>/mutants: the headers `# violates:` and
`# expect:` of <id>.patch or <id>.skip, and the reason of a skip), and PANICS (the programs directory: the
<program>.panics files, judged as lib/judge.py judges them).

Dataset `kill`, one row per program: killed is true when the program fails (how is diff, exit or
timeout). A mutant with a .skip file has no build; it records one row, program "-", how "skipped".
The job exits 0 when every row was recorded, whatever the verdicts: a kill is a result.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

from nixsci import lab

import launch
import judge


def header(path: Path, key: str) -> str:
    for line in path.read_text().splitlines():
        if line.startswith(f"# {key}:"):
            return line[len(f"# {key}:"):].strip()
    raise SystemExit(f"{path}: no '# {key}:' header")


def record(mutant: str, rule: str, expect: str, program: str, killed: bool, how: str) -> None:
    lab.record("kill", {
        "mutant": mutant, "rule": rule, "expect": expect, "program": program, "killed": killed, "how": how,
    })
    print(f"{how:8} {mutant} {program}", file=sys.stderr)


def main() -> None:
    params = lab.params()
    mutant = params["mutant"]
    timeout = float(params.get("timeout", 120))
    patches = Path(os.environ["PATCHES"])
    root = Path(os.environ["MUTANTS"]) / mutant
    if mutant == "none":
        rule, expect = "-", "control"
    else:
        meta = patches / f"{mutant}.patch"
        if not meta.is_file():
            meta = patches / f"{mutant}.skip"
        rule, expect = header(meta, "violates"), header(meta, "expect")
    if not (root / "programs").is_dir():
        record(mutant, rule, expect, "-", False, "skipped")
        return
    stderr_dir = Path(os.environ["NIX_LAB_DIR"]) / "stderr"
    stderr_dir.mkdir(parents=True, exist_ok=True)
    for path in sorted((root / "programs").iterdir()):
        name = path.name
        panic = judge.panic_line(name)
        judged = judge.expected_for(root / "share" / "expected", name, 1, panic)
        if judged is None:
            continue
        expected = judged[1]
        argv = [str(path)]
        res = launch.run(argv, {**params, "split": 1}, timeout)
        launch.record_config(res, argv)
        if res.stderr:
            (stderr_dir / f"{mutant}.{name}.txt").write_text(res.stderr)
        got = judge.normalise(res.hosts[0]) if res.hosts else ""
        how = judge.judge(res, got, expected, panic)
        record(mutant, rule, expect, name, how != "ok", how)



main()
