"""The placement of a launch on this backend, shared by `launch` and `serve`: how many workers each host is
given, how many ranks each host needs, and the mpirun command that starts them (src/backends/README.md, Launch).

Hosts are numbered machine-major: machine m holds hosts m*S .. m*S+S-1 (S = hosts per machine), and the world
ranks are numbered host by host. Each host has its own count of ranks, so the slots of a machine are the sum over
its hosts and TRAME_MPI_NODE_RANKS (transport/nodes.rs) lists the ranks of each host."""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

from nixsci.lab import mpi


class Usage(Exception):
    """The caller asked for something that cannot be launched; the message says what."""


def fail(msg: str) -> None:
    raise Usage(msg)


class NoTopology(Exception):
    """The probe run ended without asking for ranks: the program launches no topology, and that run was all of it."""

    def __init__(self, p: subprocess.CompletedProcess):
        self.p = p


def ranks_of(program: list[str], hosts: int, host: int, workers: list[int]) -> int:
    # The count is written to a file (TRAME_PROBE_OUT, backend.rs), not to stdout, which a test harness captures.
    with tempfile.TemporaryDirectory() as d:
        probe = Path(d) / "ranks"
        env = {**os.environ, "TRAME_PROBE": "ranks", "TRAME_PROBE_HOSTS": str(hosts), "TRAME_PROBE_HOST": str(host),
               "TRAME_PROBE_OUT": str(probe), "TRAME_WORKERS": ",".join(map(str, workers))}
        p = subprocess.run(["mpirun", "--oversubscribe", "-n", "1", *[a for n in ("TRAME_PROBE", "TRAME_PROBE_HOSTS",
                            "TRAME_PROBE_HOST", "TRAME_PROBE_OUT", "TRAME_WORKERS") for a in ("-x", n)], *program],
                           capture_output=True, text=True, env=env)
        text = probe.read_text() if probe.exists() else ""
    m = re.search(r"^trame-ranks (\d+)$", text, re.M)
    if not m and p.returncode == 0:
        raise NoTopology(p)
    if not m:
        sys.stderr.write(p.stdout + p.stderr)
        fail("the program did not report its rank count (is it built for this backend?)")
    return int(m.group(1))


def machine_slots() -> list[int | None]:
    """Per machine, the slots its hostfile line names (`NAME slots=N` or `NAME N`), or None."""
    path = os.environ.get("NIX_DEPLOY_HOSTFILE")
    if not path:
        return [None]
    out: list[int | None] = []
    for line in Path(path).read_text().splitlines():
        words = line.split()
        if not words:
            continue
        n = next((re.fullmatch(r"(?:slots=)?(\d+)", w) for w in words[1:]), None)
        out.append(int(n.group(1)) if n else None)
    return out


def usable_cores(machines: int) -> list[int]:
    """Per machine, the cores a process of it may run on (nproc follows the cpuset), asked on the machine itself."""
    if not os.environ.get("NIX_DEPLOY_HOSTFILE"):
        return [len(os.sched_getaffinity(0))]
    cmd = mpi.command(machines, per_node=1) + ["--output", "tag", "nproc"]
    p = subprocess.run(cmd, capture_output=True, text=True)
    found = sorted((int(m.group(1)), int(m.group(2))) for m in re.finditer(r"^\[\d+,(\d+)\]<stdout>: ?(\d+)$", p.stdout, re.M))
    if len(found) != machines:
        fail(f"could not ask the {machines} machines for their cores (nproc): {p.stderr.strip()[-300:]}")
    return [n for _, n in found]


def counts_of(text: str, hosts: int, where: str) -> list[int]:
    try:
        counts = [int(x) for x in text.split(",")]
    except ValueError:
        counts = [0]
    if min(counts) < 1 or len(counts) not in (1, hosts):
        fail(f"{where} must be one worker count of 1 or more, or one per host ({hosts}), got {text!r}")
    return counts * hosts if len(counts) == 1 else counts


def workers_of(option: str | None, machines: int, split: int) -> list[int]:
    hosts = machines * split
    if option is not None:
        return counts_of(option, hosts, "--workers")
    if os.environ.get("TRAME_WORKERS"):
        return counts_of(os.environ["TRAME_WORKERS"], hosts, "TRAME_WORKERS")
    slots = machine_slots()
    cores = usable_cores(machines) if None in slots else slots
    slots = [cores[m] if n is None else n for m, n in enumerate(slots)]
    return [max(1, slots[h // split] // split) for h in range(hosts)]


@dataclass
class Placement:
    machines: int
    hosts: int
    workers: list[int]  # trame::workers(h), per host
    ranks: list[int]  # the ranks each host declares: its workers and its leader
    env: dict[str, str]
    cmd: list[str]  # mpirun and its options; the caller appends the program


def place(program: list[str], split: int, workers_opt: str | None, bind: str, extra_export: tuple[str, ...] = ()) -> Placement:
    """Decide the workers, ask each host's ranks of the program, and build the mpirun command.
    Raises NoTopology when the program launches none, and Usage when the job cannot be placed."""
    machines = max(1, len(mpi.hosts()))
    hosts = machines * split
    workers = workers_of(workers_opt, machines, split)
    ranks = [ranks_of(program, hosts, h, workers) for h in range(hosts)]
    env, export = dict(os.environ), ["TRAME_WORKERS", *extra_export]
    env["TRAME_WORKERS"] = ",".join(map(str, workers))
    env.pop("TRAME_MPI_NODE_RANKS", None)
    if split > 1 or len(set(ranks)) > 1:
        env["TRAME_MPI_NODE_RANKS"] = ",".join(map(str, ranks))
        export.append("TRAME_MPI_NODE_RANKS")
    # mpi.command writes the hostfile with one slot count for every machine; these machines' counts differ.
    slots = [sum(ranks[m * split:(m + 1) * split]) for m in range(machines)]
    cmd = mpi.command(sum(ranks), per_node=max(slots), bind=bind, export=export)
    if "--hostfile" in cmd:
        hostfile = Path(cmd[cmd.index("--hostfile") + 1])
        hostfile.write_text("".join(f"{n} slots={s}\n" for n, s in zip(mpi.hosts(), slots)))
    return Placement(machines, hosts, workers, ranks, env, cmd)
