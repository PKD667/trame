"""Rounds through the leader (lab/lib/bench.py; program bench_round)."""

import bench


def argv(p, hosts):
    return ["bench_round", bench.g(p, "per", 4), bench.g(p, "rounds", 10000)]


bench.main(argv)
