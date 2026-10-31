"""The mpirun command line of every trame runner: nixsci.lab.mpi plus the one setting that is trame's own.

argv(ranks, params) returns the mpirun options; the caller appends the program and its arguments. On several hosts
nixsci.lab.mpi adds the hostfile, the slot placement and the launch agent. Parameters read (all optional):
  per_node    slots per host (default: ceil(ranks / hosts))
  leader      one more slot on the last host for the leader (default True)
  node_ranks  k > 0 splits one machine into nodes of k ranks (TRAME_MPI_NODE_RANKS, exported to every rank)
  bind        the --bind-to policy of mpirun (default "none": ranks float over the cores)"""

from __future__ import annotations

import os
import shlex

from nixsci import lab
from nixsci.lab import mpi

hosts = mpi.hosts
record_config = mpi.record_config


def argv(ranks: int, params: dict) -> list[str]:
    node_ranks = int(params.get("node_ranks", 0))
    export = []
    if node_ranks > 0:
        os.environ["TRAME_MPI_NODE_RANKS"] = str(node_ranks)
        export = ["TRAME_MPI_NODE_RANKS"]
    per_node = int(params["per_node"]) if params.get("per_node") else None
    return mpi.command(ranks, per_node=per_node, extra=1 if params.get("leader", True) else 0,
                       bind=str(params.get("bind", "none")), export=export)


def record_argv(cmd: list[str]) -> None:
    """The full command line of one job (mpirun, its options, the program and its arguments), one row."""
    lab.record("mpi_config", {"source": "argv", "text": shlex.join(cmd)})
