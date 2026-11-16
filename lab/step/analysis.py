import statistics

import tables as t

# ns / n is the time from one start of the step to the next; a run's value is the median over its steps.
groups = t.grouped(t.rows("step"), lambda r: (t.series(r), r["machines"], r["hosts"], r["per"], r["k"]),
                   lambda rs: statistics.median(r["ns"] / r["n"] for r in rs))
t.table("step-overhead", ["series", "machines", "hosts", "per", "k"], groups, ["ns_median", "ns_min", "ns_max"])
t.done()
