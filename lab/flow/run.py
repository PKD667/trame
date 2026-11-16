"""Bandwidth of one stream per worker (lab/lib/bench.py; program bench_flow)."""

import bench

g = bench.g


def argv(p, hosts):
    a = ["bench_flow", g(p, "pattern", "ring"), g(p, "per", 4), g(p, "size", 64), g(p, "cap", 8), g(p, "steps", 20000)]
    return a + ["trace"] if g(p, "trace", 0) else a


bench.main(argv)
