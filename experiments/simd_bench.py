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
    args = [f"{os.environ.get('BIN', '')}/bench"]
    if table_name == "bench_knee":
        args.append("knee")
    
    try:
        res = proc.run(["taskset", "-c", "4"] + args, timeout=600.0)
    except Exception as e:
        lab.record("error", {"msg": f"proc.run {table_name}: {e}"})
        return []
    
    if res.rc != 0:
        err_msg = f"bench {table_name} rc={res.rc}"
        if res.stderr:
            err_msg += f" stderr={res.stderr[:100]}"
        lab.record("error", {"msg": err_msg})
        return []
    
    # Get hostname for the machine field
    hostname = subprocess.run(["hostname", "-f"], capture_output=True, text=True).stdout.strip()
    
    rows = []
    for line in res.stdout.splitlines():
        if line.startswith("#") or not line.strip():
            continue
        fields = line.split("\t")
        if len(fields) != len(BENCH_COLUMNS):
            raise SystemExit(f"bad row in {table_name}: {line!r} (expected {len(BENCH_COLUMNS)} fields)")
        
        r = dict(zip(BENCH_COLUMNS, fields))
        mitems_s = float(r["mitems_s"])
        rows.append({
            "machine": hostname,
            "table": table_name,
            "case": r["case"],
            "dist": r["dist"],
            "items": int(r["items"]),
            "slots": int(r["slots"]),
            "variant": r["variant"],
            "median_mitems_s": mitems_s,
            "min_mitems_s": mitems_s,  # For now, use same value across all three
            "max_mitems_s": mitems_s,
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
    lab.record("debug", {"msg": "main started"})
    
    # Get experiment parameters (required by nixsci)
    params = lab.params()
    lab.record("debug", {"msg": "params read"})
    
    # Record machine info once per job.
    info = machine_info()
    lab.record("machine", info)
    lab.record("debug", {"msg": "machine info recorded"})
    
    # Debug: check what's in BIN directory
    bin_dir = os.environ.get('BIN', 'BIN_NOT_SET')
    try:
        if bin_dir != 'BIN_NOT_SET':
            bin_contents = os.listdir(bin_dir)
            lab.record("debug", {"msg": f"bin_dir={bin_dir} has {len(bin_contents)} items"})
        else:
            lab.record("debug", {"msg": f"BIN env var not set"})
    except Exception as e:
        lab.record("error", {"msg": f"BIN error: {e}"})
        # Don't re-raise, continue
    
    # Run bench table.
    lab.record("debug", {"msg": "before_bench"})
    bench_rows = run_bench("bench")
    lab.record("debug", {"msg": f"bench done: {len(bench_rows)} rows"})
    for i, row in enumerate(bench_rows):
        try:
            lab.record("bench", row)
        except Exception as e:
            lab.record("error", {"msg": f"record bench row {i}: {e}"})
            if i < 3 or i % 50 == 0:
                lab.record("debug", {"msg": f"recorded bench row {i} ok"})
            break  # Stop if recording fails
    
    # Run knee table.
    lab.record("debug", {"msg": "before_knee"})
    knee_rows = run_bench("bench_knee")
    lab.record("debug", {"msg": f"knee done: {len(knee_rows)} rows"})
    for i, row in enumerate(knee_rows):
        try:
            lab.record("bench", row)  # Both go into the bench dataset.
        except Exception as e:
            lab.record("error", {"msg": f"record knee row {i}: {e}"})
            break


if __name__ == "__main__":
    main()
