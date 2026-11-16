"""Fan-in: every worker sends MSGS messages to the leader (lab/lib/bench.py; program bench_fanin)."""

import bench


def argv(p, hosts):
    return ["bench_fanin", bench.g(p, "per", 4), bench.g(p, "msgs", 10000)]


bench.main(argv)
