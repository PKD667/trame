"""One launch of the one-warp GPU prototype per job (backends/nv/PLAN.md, gate D; gpu/README.md).

Environment (set by backends/nv/flake.nix): HOST (nv-post-parallel-host), PTX (the PTX cargo-oxide
built from gpu/src/main.rs), EXPECTED (programs/post_parallel_pod.expected), DRIVER (nixsci's
driver.sh: binds the host's libcuda.so.1 for one command). The experiment has `replicates = 3`: each
job is one fresh process, so each run is one fresh driver context.

Datasets, one row per job:
  run  ran (bool), ok (bool), verdict, device, compute_capability, driver_version (int?), ptx_sha256,
       stdout_sha256 (of the host's stdout), seq_sha256 (of the seq reference, the .expected file),
       exit_code (int).
       verdict: ok (stdout equal to the expected text), diff (ran, other stdout), no-driver (host exit 3:
       no libcuda.so.1, nothing measured), cuda-error (host exit 4 or 2: a CUDA call or the arguments
       failed), timeout.
       ran is true only when the kernel launched and its output was copied back (host exit 0). A row
       with ran false says the apparatus did not get as far as measuring.
  smi  ran (bool), name, driver_version, compute_cap, memory_total: nvidia-smi on the node, the raw
       `nvidia-smi -q` as an artifact.

No timing is recorded: this is a semantics test (PLAN.md).
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from nixsci import lab

TIMEOUT = 120.0


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def normalise(stdout: str) -> str:
    """As programs.py: trailing newlines dropped, one newline per line."""
    text = stdout.rstrip("\n")
    return "".join(line + "\n" for line in text.split("\n")) if text else ""


def bind_driver(driver_sh: str, workdir: Path) -> str:
    """nixsci's driver.sh: prints the LD_LIBRARY_PATH that finds the host's driver (one command only)."""
    r = subprocess.run([driver_sh, str(workdir)], capture_output=True, text=True)
    sys.stderr.write(r.stderr)
    return r.stdout.strip()


def device_line(stderr: str) -> dict:
    """The host's first JSON line on stderr, or {} when the host printed none."""
    for line in stderr.splitlines():
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                return {}
    return {}


def smi() -> None:
    """nvidia-smi: identity of the GPU as the driver reports it, and the raw query."""
    exe = shutil.which("nvidia-smi") or ("/usr/bin/nvidia-smi" if os.path.exists("/usr/bin/nvidia-smi") else None)
    if exe is None:
        lab.record("smi", {"ran": False, "name": "-", "driver_version": "-", "compute_cap": "-", "memory_total": "-"})
        return
    q = subprocess.run(
        [exe, "--query-gpu=name,driver_version,compute_cap,memory.total", "--format=csv,noheader"],
        capture_output=True, text=True,
    )
    raw = subprocess.run([exe, "-q"], capture_output=True, text=True)
    lab.record("smi_raw", raw.stdout.encode() or raw.stderr.encode() or b"(empty)")
    fields = [f.strip() for f in q.stdout.splitlines()[0].split(",")] if q.returncode == 0 and q.stdout.strip() else []
    ran = q.returncode == 0 and len(fields) == 4
    lab.record("smi", {
        "ran": ran,
        "name": fields[0] if ran else "-",
        "driver_version": fields[1] if ran else "-",
        "compute_cap": fields[2] if ran else "-",
        "memory_total": fields[3] if ran else "-",
    })


def main() -> None:
    host = os.environ["HOST"]
    ptx = Path(os.environ["PTX"])
    expected_text = Path(os.environ["EXPECTED"]).read_text()
    lab_dir = Path(os.environ["NIX_LAB_DIR"])
    workdir = lab_dir.parent

    ptx_sha = sha(ptx.read_bytes())
    seq_sha = sha(expected_text.encode())
    smi()

    ld = bind_driver(os.environ["DRIVER"], workdir)
    env = {**os.environ, "LD_LIBRARY_PATH": ld} if ld else dict(os.environ)
    try:
        res = subprocess.run([host, str(ptx)], capture_output=True, text=True, timeout=TIMEOUT, env=env)
    except subprocess.TimeoutExpired as e:
        stderr = (e.stderr or b"").decode(errors="replace") if isinstance(e.stderr, bytes) else (e.stderr or "")
        (lab_dir / "host-stderr.txt").write_text(stderr)
        row = {"ran": False, "ok": False, "verdict": "timeout", "device": "-", "compute_capability": "-",
               "driver_version": None, "ptx_sha256": ptx_sha, "stdout_sha256": sha(b""), "seq_sha256": seq_sha,
               "exit_code": -1}
        lab.record("run", row)
        print("timeout", file=sys.stderr)
        return

    (lab_dir / "host-stderr.txt").write_text(res.stderr)
    dev = device_line(res.stderr)
    got = normalise(res.stdout)
    if res.returncode == 0:
        verdict = "ok" if got == expected_text else "diff"
    elif res.returncode == 3:
        verdict = "no-driver"
    else:
        verdict = "cuda-error"
    row = {
        "ran": res.returncode == 0,
        "ok": verdict == "ok",
        "verdict": verdict,
        "device": str(dev.get("device", "-")),
        "compute_capability": str(dev.get("compute_capability", "-")),
        "driver_version": dev.get("driver_version"),
        "ptx_sha256": ptx_sha,
        "stdout_sha256": sha(got.encode()),
        "seq_sha256": seq_sha,
        "exit_code": res.returncode,
    }
    lab.record("run", row)
    print(f"{verdict} rc={res.returncode} device={row['device']} cc={row['compute_capability']}", file=sys.stderr)
    if verdict == "diff":
        print("--- got\n" + got + "--- expected\n" + expected_text, file=sys.stderr)


if __name__ == "__main__":
    main()
