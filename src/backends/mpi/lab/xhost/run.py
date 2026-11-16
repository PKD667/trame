"""The cross-machine transport microbenchmark (xhost/main.rs): one job, one mode, two ranks on two machines.

Parameters: mode (direct, direct_single, relay, mixed_self, mixed_flag, wake_futex), iters (measured
iterations, default 100000), warmup (default 10000), sizes (comma list of bytes, default 8,64,1024,65536),
mca (Open MPI MCA settings, space-separated name=value, default none), timeout (seconds, default 1800),
same_host (default false: a job whose two ranks land on one machine is refused, since it measures shared
memory and not a network). Environment: BIN (the binary), RUSTC_VERSION and OPENMPI_VERSION (what the
closure was built with), NIX_LAB_DIR.

Placement: one slot per host, rank 0 on the first host of the job, rank 1 on the second (lib/mpirun.py, nixsci.lab.mpi).

Dataset `latency`, one row per (mode, phase, size). Times are nanoseconds. `phase` rtt: the latency is half
the round trip, `size` the frame's bytes. `phase` wake: the latency of a local wake (size 1); the rows
of mixed_self, mixed_flag and wake_futex. `cpu0_ns` and `cpu1_ns` are the CPU time (user + system) of rank 0
and rank 1 per iteration; `sys0_ns` and `sys1_ns` the system part (the split is the kernel's estimate).
For wake rows the CPU is per wake over the warm-up and measured wakes together, pauses included.

Each job also writes `mpi_config` rows: lib/mpirun.py's, and the host of each rank, the CPU model, the network
interfaces with their speed, the InfiniBand/Omni-Path devices, the compiler and Open MPI the closure was built
with, the MCA settings of this job, the MPI library string that rank 0 printed, and `transport` rows: the selection
lines of a short probe run of the same placement with Open MPI's pml, btl and mtl selection verbose.
"""

from __future__ import annotations

import os
import re
from pathlib import Path

from nixsci import lab

import mpirun
from nixsci.lab import proc

COLUMNS = ["mode", "phase", "size", "iters", "median_ns", "p99_ns", "min_ns", "mean_ns", "cpu0_ns", "cpu1_ns", "sys0_ns", "sys1_ns"]
MODES = ["direct", "direct_single", "relay", "mixed_self", "mixed_flag", "wake_futex"]
RANK_LINE = re.compile(r"^\[\d+,(\d+)\]<(stdout|stderr)>: ?(.*)$")  # mpirun 5 writes a space after the colon


def machine() -> list[tuple[str, str]]:
    """What this machine says about its CPU and network, as (source, text). Read from /proc and /sys, on the
    first host of the job; a host without InfiniBand has no `infiniband` row."""
    rows = []
    model = [l.split(":", 1)[1].strip() for l in Path("/proc/cpuinfo").read_text().splitlines() if l.startswith("model name")]
    if model:
        rows.append(("cpu", f"{model[0]} x{len(model)} threads"))
    route = [l.split() for l in Path("/proc/net/route").read_text().splitlines()[1:]]
    default = next((r[0] for r in route if r[1] == "00000000"), "")
    for dev in sorted(Path("/sys/class/net").iterdir()):
        speed = ""
        try:
            speed = (dev / "speed").read_text().strip()
        except OSError:
            pass
        rows.append(("net", f"{dev.name} speed_mbps={speed or '?'}{' default-route' if dev.name == default else ''}"))
    ib = Path("/sys/class/infiniband")
    for dev in sorted(ib.iterdir()) if ib.is_dir() else []:
        for port in sorted((dev / "ports").glob("*")):
            rate = (port / "rate").read_text().strip() if (port / "rate").exists() else "?"
            link = (port / "link_layer").read_text().strip() if (port / "link_layer").exists() else "?"
            rows.append(("infiniband", f"{dev.name} port {port.name} rate={rate} link_layer={link}"))
    return rows


def parse(stdout: str) -> list[dict]:
    """The TSV rows of rank 0 (mpirun tags every line with [job,rank]<stream>:)."""
    lines = []
    for text in stdout.splitlines():
        m = RANK_LINE.match(text)
        if m and m.group(2) == "stdout":
            lines.append(m.group(3))
    header = [i for i, l in enumerate(lines) if l.split("\t") == COLUMNS]
    if len(header) != 1:
        raise SystemExit(f"expected one table header, found {len(header)}")
    rows = []
    for l in lines[header[0] + 1:]:
        f = l.split("\t")
        if len(f) != len(COLUMNS):
            raise SystemExit(f"bad row: {l!r}")
        r = dict(zip(COLUMNS, f))
        rows.append({
            "mode": r["mode"], "phase": r["phase"], "size": int(r["size"]), "iters": int(r["iters"]),
            **{k: float(r[k]) for k in COLUMNS[4:]},
        })
    return rows


def probe_transport(base: list[str], binary: str) -> None:
    """Which MPI transport the job's two hosts get. A short run of the same placement with Open MPI's selection
    verbose, its selection lines recorded as `transport` rows (at most 40, 200 characters each). A probe that fails
    records a row saying so and the job goes on: the probe is a record, not a check."""
    cmd = base + ["--output", "tag", "--mca", "pml_base_verbose", "10", "--mca", "btl_base_verbose", "10",
                 "--mca", "mtl_base_verbose", "10", binary, "direct", "--iters", "200", "--warmup", "50", "--sizes", "8"]
    res = proc.run(cmd, 300.0)
    if res.timed_out or res.rc != 0:
        lab.record("mpi_config", {"source": "transport", "text": f"probe failed: rc={res.rc}"})
        return
    seen: list[str] = []
    for line in (res.stdout + res.stderr).splitlines():
        m = RANK_LINE.match(line)
        body = (m.group(3) if m else line).strip()
        if re.search(r"select|selected|component|initializ|using|transport|provider", body, re.I) and body not in seen:
            seen.append(body)
    for body in seen[:40]:
        lab.record("mpi_config", {"source": "transport", "text": body[:200]})


def main() -> None:
    p = lab.params()
    mode = p["mode"]
    if mode not in MODES:
        raise SystemExit(f"unknown mode {mode!r}")
    hosts = mpirun.hosts()
    if len(hosts) < 2 and not p.get("same_host", False):
        raise SystemExit("xhost measures a network: it needs a job on two hosts (span = 2)")
    mpirun.record_config()
    for source, text in machine():
        lab.record("mpi_config", {"source": source, "text": text})
    for name in ("RUSTC_VERSION", "OPENMPI_VERSION"):
        lab.record("mpi_config", {"source": name.lower(), "text": os.environ.get(name, "")})
    mca = str(p.get("mca", "")).split()
    for setting in mca:
        lab.record("mpi_config", {"source": "mca", "text": setting})
    cmd = mpirun.argv(2, {"per_node": 1, "leader": False, "bind": p.get("bind", "none")})
    for setting in mca:
        name, _, value = setting.partition("=")
        cmd += ["--mca", name, value]
    cmd += ["--output", "tag", str(Path(os.environ["BIN"]) / "mpi_cross_host"), mode,
            "--iters", str(p.get("iters", 100000)), "--warmup", str(p.get("warmup", 10000)),
            "--sizes", str(p.get("sizes", "8,64,1024,65536"))]
    mpirun.record_argv(cmd)
    probe_transport(cmd[: cmd.index("--output")], str(Path(os.environ["BIN"]) / "mpi_cross_host"))
    res = proc.run(cmd, float(p.get("timeout", 1800)))
    logs = Path(os.environ["NIX_LAB_DIR"]) / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    (logs / f"{mode}.stdout").write_text(res.stdout)
    (logs / f"{mode}.stderr").write_text(res.stderr)
    if res.timed_out or res.rc != 0:
        raise SystemExit(f"{mode}: {'timeout' if res.timed_out else f'mpirun exit {res.rc}'}; see logs/{mode}.stderr")
    placed = {}
    for text in res.stderr.splitlines():
        m = RANK_LINE.match(text)
        if m and m.group(2) == "stderr" and m.group(3).startswith("# rank "):
            f = m.group(3).split()
            placed[int(f[2])] = f[4]
            if int(f[2]) == 0:
                lab.record("mpi_config", {"source": "library", "text": " ".join(f[9:])})
    if len(placed) != 2:
        raise SystemExit(f"{mode}: {len(placed)} ranks reported a host")
    for rank in sorted(placed):
        lab.record("mpi_config", {"source": f"rank{rank}-host", "text": placed[rank]})
    if placed[0] == placed[1] and not p.get("same_host", False):
        raise SystemExit(f"{mode}: both ranks ran on {placed[0]}")
    for row in parse(res.stdout):
        lab.record("latency", row)
        print(row, flush=True)


if __name__ == "__main__":
    main()
