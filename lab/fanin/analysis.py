import statistics

import tables as t


def per_count(rs):
    """Median over the rows that name a worker of ns per msgs (a null worker is not a row of the benchmark)."""
    live = [r for r in rs if r.get("worker") is not None]
    return statistics.median(r["ns"] / r["msgs"] for r in live) if live else None


groups = t.grouped(t.rows("fanin"), lambda r: (t.series(r), r["machines"], r["hosts"], r["workers"]), per_count)
t.table("fanin", ["series", "machines", "hosts", "workers"], groups, ["ns_per_msg_median", "ns_per_msg_min", "ns_per_msg_max"])
t.done()
