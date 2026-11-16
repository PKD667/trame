import statistics

import tables as t

data = t.rows("graph")
key = lambda r: (t.series(r), r["machines"], r["hosts"], r["workers"], r["size"])
groups = t.grouped(data, key, lambda rs: statistics.median(r["ns"] / r["steps"] for r in rs))
share = t.grouped(data, key, lambda rs: statistics.median(r["compute_ns"] / r["ns"] for r in rs))
lines = []
for k, per_run in groups.items():
    n, med, lo, hi = t.summary(per_run)
    lines.append([*k, n, med, lo, hi, t.summary(share[k])[1]])
t.write("graph", ["series", "machines", "hosts", "workers", "size", "runs", "ns_per_step_median", "ns_per_step_min", "ns_per_step_max",
                  "compute_share_median"], lines)
t.done()
