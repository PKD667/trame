"""SIMD benchmark experiment runner: executes bench and bench knee binaries and records TSV results.

Parameters: cpu_type (zen1, intel-avx512, zen2plus). Environment: BIN (the binary), RUSTC_VERSION.

Runs `taskset -c 4 bench` and `taskset -c 4 bench knee`, parses TSV output (columns: case, dist,
items, slots, variant, median_ns, Mitems/s, eq), records them as typed rows with table name
(bench or bench_knee), and records one machine row per job with hostname, lscpu model name, AVX2/AVX512
flags, kernel version, and rustc.
"""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

from nixsci import lab

import proc

BENCH_COLUMNS = ["case", "dist", "items", "slots", "variant", "median_ns", "mitems_s", "eq"]
MACHINE_COLUMNS = ["hostname", "model", "avx2", "avx512f", "kernel", "rustc"]


def run_bench(table_name: str) -> list[dict]:
    """Run bench or bench knee, parse TSV output, return typed rows."""
    args = [f"{os.environ['BIN']}/bench"]
    if table_name == "bench_knee":
        args.append("knee")
    
    res = proc.run(["taskset", "-c", "4"] + args, timeout=600.0)
    
    if res.rc != 0:
        import sys
        print(f"ERROR: bench failed with rc {res.rc}", file=sys.stderr)
        print(f"stderr: {res.stderr}", file=sys.stderr)
        print(f"stdout: {res.stdout}", file=sys.stderr)
        raise SystemExit(f"bench failed with rc {res.rc}: {res.stderr}")
    
    rows = []
    for line in res.stdout.splitlines():
        if line.startswith("#") or not line.strip():
            continue
        fields = line.split("\t")
        if len(fields) != len(BENCH_COLUMNS):
            raise SystemExit(f"bad row in {table_name}: {line!r} (expected {len(BENCH_COLUMNS)} fields)")
        
        r = dict(zip(BENCH_COLUMNS, fields))
        rows.append({
            "table": table_name,
            "case": r["case"],
            "dist": r["dist"],
            "items": int(r["items"]),
            "slots": int(r["slots"]),
            "variant": r["variant"],
            "median_ns": int(r["median_ns"]),
            "mitems_s": float(r["mitems_s"]),
            "eq": r["eq"],
        })
    
    return rows


def machine_info() -> dict:
    """Hostname, lscpu model name, AVX2/AVX512F flags, kernel version, rustc version."""
    hostname = subprocess.run(["hostname", "-f"], capture_output=True, text=True).stdout.strip()
    
    cpuinfo = Path("/proc/cpuinfo").read_text()
    model = "unknown"
    for line in cpuinfo.splitlines():
        if line.startswith("model name"):
            model = line.split(":", 1)[1].strip()
            break
    
    flags = set()
    for line in cpuinfo.splitlines():
        if line.startswith("flags"):
            flags = set(line.split(":", 1)[1].split())
            break
    
    kernel = subprocess.run(["uname", "-r"], capture_output=True, text=True).stdout.strip()
    rustc = os.environ.get("RUSTC_VERSION", "unknown")
    
    return {
        "hostname": hostname,
        "model": model,
        "avx2": "avx2" in flags,
        "avx512f": "avx512f" in flags,
        "kernel": kernel,
        "rustc": rustc,
    }


def main():
    # Record machine info once per job.
    info = machine_info()
    lab.record("machine", info)
    
    # Run bench table.
    bench_rows = run_bench("bench")
    for row in bench_rows:
        lab.record("bench", row)
    
    # Run knee table.
    knee_rows = run_bench("bench_knee")
    for row in knee_rows:
        lab.record("bench", row)  # Both go into the bench dataset.


if __name__ == "__main__":
    main()
