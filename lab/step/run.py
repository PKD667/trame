"""Step overhead: a step that does nothing, repeated N times (lab/lib/bench.py; program bench_step)."""

import bench


def argv(p, hosts):
    return ["bench_step", bench.g(p, "per", 4), bench.g(p, "k", 1), bench.g(p, "n", 1000000)]


bench.main(argv)
