"""The mpirun command line of every trame runner, and the MPI configuration each job runs under.

argv(ranks, params) returns the mpirun options. The caller appends the program and its arguments.

One host: `mpirun --oversubscribe -n RANKS`.
Several hosts (nixsci.deploy sets NIX_DEPLOY_HOSTFILE for a job with span > 1): a hostfile of this
run's slots, ranks placed by slot, and a launch agent that reaches the other hosts through
NIX_DEPLOY_RSH and runs inside their store view with NIX_DEPLOY_ENTER (OMPI_MCA_plm_rsh_agent).

Parameters read (all optional):
  per_node    slots per host (default: ceil(ranks / hosts))
  leader      one more slot on the last host for the leader (default True)
  node_ranks  k > 0 splits one machine into nodes of k ranks (TRAME_MPI_NODE_RANKS, exported to every rank)

MPI transport settings are machine configuration, not repository content: record_config() writes
them into the job's dataset mpi_config, so a run says what it ran under.
"""

from __future__ import annotations

import os
from pathlib import Path

from nixsci import lab

import proc

# The MCA parameter of OpenMPI's launch agent for remote daemons (rsh/ssh launch).
AGENT_MCA = "plm_rsh_agent"

# Verbatim from the orchestrator's verified wrapper: the two variables are command prefixes, unquoted on purpose.
AGENT_SCRIPT = '#!/bin/sh\nhost="$1"; shift; exec $NIX_DEPLOY_RSH "$host" $NIX_DEPLOY_ENTER "$@"\n'


def _workdir() -> Path:
    return Path(os.environ.get("NIX_DEPLOY_WORKDIR") or os.getcwd())


def _hosts() -> list[str]:
    path = os.environ.get("NIX_DEPLOY_HOSTFILE")
    if not path:
        return []
    return [h.strip() for h in Path(path).read_text().splitlines() if h.strip()]


def _agent() -> str:
    path = _workdir() / "rsh-agent"
    path.write_text(AGENT_SCRIPT)
    path.chmod(0o755)
    return str(path)


def _hostfile(hosts: list[str], ranks: int, params: dict) -> str:
    per_node = int(params.get("per_node") or -(-ranks // len(hosts)))
    slots = [per_node] * len(hosts)
    if params.get("leader", True):
        slots[-1] += 1
    path = _workdir() / "hostfile"
    path.write_text("".join(f"{h} slots={s}\n" for h, s in zip(hosts, slots)))
    return str(path)


def argv(ranks: int, params: dict) -> list[str]:
    """mpirun options for `ranks` ranks; the caller appends the program and its arguments."""
    cmd = ["mpirun", "--oversubscribe", "-n", str(ranks)]
    hosts = _hosts()
    if hosts:
        cmd += [
            "--hostfile", _hostfile(hosts, ranks, params),
            "--map-by", "slot", "--bind-to", "core", "--report-bindings",
            "--prtemca", AGENT_MCA, _agent(),
        ]
    node_ranks = int(params.get("node_ranks", 0))
    if node_ranks > 0:
        os.environ["TRAME_MPI_NODE_RANKS"] = str(node_ranks)
        cmd += ["-x", "TRAME_MPI_NODE_RANKS"]
    return cmd


def record_config() -> None:
    """One mpi_config row per item: the two MCA files of the account, mpirun's version, each MCA variable."""
    home = Path(os.environ.get("HOME", "/nonexistent"))
    for source, path in (
        ("home-openmpi", home / ".openmpi" / "mca-params.conf"),
        ("home-prte", home / ".prte" / "mca-params.conf"),
    ):
        text = path.read_text() if path.is_file() else ""
        lab.record("mpi_config", {"source": source, "text": text})
    lines = proc.run(["mpirun", "--version"], 30.0).stdout.splitlines()
    lab.record("mpi_config", {"source": "version", "text": lines[0] if lines else ""})
    for name in sorted(os.environ):
        if name.startswith(("OMPI_MCA_", "PRTE_MCA_")):
            lab.record("mpi_config", {"source": "env", "text": f"{name}={os.environ[name]}"})
