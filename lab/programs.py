"""Every program of the table against its .expected output: as check/programs/run.sh checks them.

Parameters: backend (seq or mpi), repeats (default 1), timeout (seconds, default 120), split (hosts per
machine, default 1), and mpirun.py's bind. Environment (set by flake.nix): BIN (program binaries),
EXPECTED (<program>.expected files), RANKS_FILE (the table `<program> <ranks>`), and PANICS (optional:
the directory of <program>.panics files, the programs that must panic, as check/programs/run.sh).

Dataset `run`, one row per program and repeat. verdict is one of
  ok       exit 0 and stdout equal to the expected text
  diff     exit 0, other stdout
  exit     nonzero exit status (a program with a .panics file: zero status, or no panic line on stderr)
  timeout  the timeout ended it
On mpi a host is one machine's ranks, or a split of them (TRAME_MPI_NODE_RANKS, src/backends/mpi/hosts.rs), and
every host runs the whole program with its own leader: a job of H hosts starts H times the program's
ranks, each line tagged with its rank (--output tag). A host's output is its ranks' lines in rank order, with
consecutive duplicate lines collapsed (refuse prints once per rank), as check/programs/run.sh. With one host
it must equal <name>.expected. With H hosts: the hosts' outputs one after the other must equal
<name>.h<H>.expected when it exists; a program that has an .h<K>.expected for another K only (it is
written for K hosts) is not run; any other program's every host must equal <name>.expected.
stdout_sha256 hashes the text compared (host 0's, when each host is compared alone), so it is equal to
the seq run's for the same program.
The job exits 0 when every run was recorded, whatever the verdicts: a failed program is a result.

mutants.py imports panic_line, read_expected and judge from here: one rule for both runners.
"""

from __future__ import annotations

import hashlib
import os
import re
import sys
from pathlib import Path

from nixsci import lab

import mpirun
from nixsci.lab import mpi, proc


def programs() -> list[tuple[str, int]]:
    rows = []
    for line in Path(os.environ["RANKS_FILE"]).read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            name, ranks = line.split()
            rows.append((name, int(ranks)))
    return rows


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


def normalise(stdout: str, collapse: bool) -> str:
    """The text that is compared: trailing newlines dropped, one newline per line."""
    text = stdout.rstrip("\n")
    lines = text.split("\n") if text else []
    if collapse:
        lines = [line for i, line in enumerate(lines) if i == 0 or line != lines[i - 1]]
    return "".join(line + "\n" for line in lines)


def judge(res: proc.Result, got: str, expected: str | None, panic: str | None) -> str:
    """The verdict of one run, as check/programs/run.sh: ok, diff, exit or timeout (the module docstring)."""
    if res.timed_out:
        return "timeout"
    if panic is not None:
        # A program that must panic: non-zero status, and the panic line on stderr. Its stdout is judged
        # only when it has an .expected file.
        if res.rc == 0 or panic not in [ERR_TAG.sub("", line) for line in res.stderr.split("\n")]:
            return "exit"
        return "ok" if expected is None or got == expected else "diff"
    if res.rc != 0:
        return "exit"
    return "ok" if got == expected else "diff"


def hosts_of_job(params: dict) -> tuple[int, int]:
    """(machines, hosts): the job's machines, and the trame hosts on them (split per machine)."""
    machines = max(1, len(mpirun.hosts()))
    return machines, machines * int(params.get("split", 1))


def command(backend: str, binary: str, ranks: int, params: dict) -> list[str]:
    """seq: the binary. mpi: every host gets `ranks` ranks, a machine `split` hosts of them."""
    if backend == "seq":
        return [binary]
    split = int(params.get("split", 1))
    _, hosts = hosts_of_job(params)
    export = []
    if split > 1:
        os.environ["TRAME_MPI_NODE_RANKS"] = str(ranks)
        export = ["TRAME_MPI_NODE_RANKS"]
    else:
        os.environ.pop("TRAME_MPI_NODE_RANKS", None)
    words = mpi.command(ranks * hosts, per_node=ranks * split, bind=str(params.get("bind", "none")), export=export)
    return words + ["--output", "tag", binary]


ERR_TAG = re.compile(r"^\[\d+,\d+\]<stderr>: ?")  # the same tag on stderr, dropped before the panic line is sought
RANK_LINE = re.compile(r"^\[\d+,(\d+)\]<stdout>: ?(.*)$")  # mpirun --output tag; 5 writes a space after the colon


def host_outputs(stdout: str, ranks: int, hosts: int) -> list[str]:
    """Each host's stdout: its ranks' lines (mpirun --output tag) in rank order, each rank's in its own
    order, consecutive duplicates collapsed. Tagged lines reach mpirun from every machine, unlike
    --output filename, whose files stay on the machine of the rank."""
    per_rank: dict[int, list[str]] = {}
    for line in stdout.split("\n"):
        m = RANK_LINE.match(line)
        if m:
            per_rank.setdefault(int(m.group(1)), []).append(m.group(2))
    texts = []
    for h in range(hosts):
        lines = [x for k in range(h * ranks, (h + 1) * ranks) for x in per_rank.get(k, [])]
        texts.append(normalise("".join(x + "\n" for x in lines), True))
    return texts


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
    backend = params.get("backend", "seq")
    if backend not in ("seq", "mpi"):
        raise SystemExit(f"backend must be seq or mpi, not {backend!r}")
    repeats = int(params.get("repeats", 1))
    timeout = float(params.get("timeout", 120))
    bindir = Path(os.environ["BIN"])
    expected_dir = Path(os.environ["EXPECTED"])
    stderr_dir = Path(os.environ["NIX_LAB_DIR"]) / "stderr"
    stderr_dir.mkdir(parents=True, exist_ok=True)
    hosts = hosts_of_job(params)[1] if backend == "mpi" else 1
    if backend == "mpi":
        mpirun.record_config()
    verdicts: dict[str, int] = {}
    for name, ranks in programs():
        panic = panic_line(name)
        how = expected_for(expected_dir, name, hosts, panic)
        if how is None:
            print(f"skip    {backend} {name}: written for another host count than {hosts}", file=sys.stderr)
            continue
        mode, expected = how
        for repeat in range(1, repeats + 1):
            res = proc.run(command(backend, str(bindir / name), ranks, params), timeout)
            if res.stderr:
                (stderr_dir / f"{name}.{repeat}.txt").write_text(res.stderr)
            if backend == "seq":
                got = normalise(res.stdout, False)
                verdict = judge(res, got, expected, panic)
            else:
                each = host_outputs(res.stdout, ranks, hosts)
                if mode == "all":
                    got = "".join(each)
                    verdict = judge(res, got, expected, panic)
                else:
                    got = each[0]
                    verdict = next((v for v in (judge(res, g, expected, panic) for g in each) if v != "ok"), "ok")
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
