import statistics

import tables as t


def per_count(rs):
    """Median over the rows that name a worker of ns per rounds (a null worker is not a row of the benchmark)."""
    live = [r for r in rs if r.get("worker") is not None]
    return statistics.median(r["ns"] / r["rounds"] for r in live) if live else None


groups = t.grouped(t.rows("round"), lambda r: (t.series(r), r["machines"], r["hosts"], r["workers"]), per_count)
t.table("round", ["series", "machines", "hosts", "workers"], groups, ["ns_per_round_median", "ns_per_round_min", "ns_per_round_max"])
t.done()
