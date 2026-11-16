"""Idle CPU: workers that wait for SECONDS seconds (lab/lib/bench.py; program bench_idle)."""

import bench


def argv(p, hosts):
    return ["bench_idle", bench.g(p, "per", 4), bench.g(p, "seconds", 3)]


bench.main(argv)
