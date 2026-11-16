"""Neighbour exchange on a circulant graph (lab/lib/bench.py; program bench_graph).

A job with PER workers or fewer per host than the degree has no such graph: it records nothing."""

import bench

g = bench.g


def argv(p, hosts):
    per, degree = g(p, "per", 4), g(p, "degree", 8)
    if per <= degree:
        return None
    return ["bench_graph", per, g(p, "graph_steps", 2000), g(p, "size", 64), degree, g(p, "graph_iters", 100000),
            g(p, "seed", 1)]


bench.main(argv)
