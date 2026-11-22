"""The mpirun command line of the trame runners that place ranks by count (scale, transport): nixsci.lab.mpi.

argv(ranks, params) returns the mpirun options; the caller appends the program and its arguments. On several hosts
nixsci.lab.mpi adds the hostfile, the slot placement and the launch agent. Parameters read (all optional):
  per_node    slots per host (default: ceil(ranks / hosts))
  leader      one more slot on the last host for the leader (default True)
  bind        the --bind-to policy of mpirun (default "none": ranks float over the cores)"""

from __future__ import annotations

import os
import shlex
import subprocess
from pathlib import Path

from nixsci import lab
from nixsci.lab import mpi

hosts = mpi.hosts


def record_config() -> None:
    """Rows of `launch_config` saying what MPI setup this run had: the account's MCA files, mpirun's version, each MCA
    variable of the environment, and the load average at the start. nixsci.lab.mpi.record_config says the same under
    the dataset `mpi_config`, which the experiments here do not declare (they declare `launch_config`, as the programs do)."""
    home = Path(os.environ.get("HOME", "/nonexistent"))
    for source, path in (("home-openmpi", home / ".openmpi" / "mca-params.conf"),
                         ("home-prte", home / ".prte" / "mca-params.conf")):
        lab.record("launch_config", {"source": source, "text": path.read_text() if path.is_file() else ""})
    version = subprocess.run(["mpirun", "--version"], capture_output=True, text=True, check=False).stdout
    lab.record("launch_config", {"source": "version", "text": version.strip().splitlines()[0] if version.strip() else ""})
    lab.record("launch_config", {"source": "loadavg", "text": Path("/proc/loadavg").read_text().strip()})
    for name in sorted(os.environ):
        if name.startswith(("OMPI_MCA_", "PRTE_MCA_")):
            lab.record("launch_config", {"source": "env", "text": f"{name}={os.environ[name]}"})


def argv(ranks: int, params: dict) -> list[str]:
    per_node = int(params["per_node"]) if params.get("per_node") else None
    return mpi.command(ranks, per_node=per_node, extra=1 if params.get("leader", True) else 0,
                       bind=str(params.get("bind", "none")))


def record_argv(cmd: list[str]) -> None:
    """The full command line of one job (mpirun, its options, the program and its arguments), one row."""
    lab.record("launch_config", {"source": "argv", "text": shlex.join(cmd)})
