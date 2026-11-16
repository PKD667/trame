import statistics

import tables as t

# One row per check and worker count: the runs, how many held, the median seconds.
by = {}
for r in t.rows("check"):
    by.setdefault((r["alias"], r["check"], r["workers"]), []).append(r)
lines = [[alias, check, workers, len(rs), sum(1 for r in rs if r["ok"]), statistics.median(r["seconds"] for r in rs)]
         for (alias, check, workers), rs in by.items()]
t.write("transport-checks", ["run", "check", "workers", "runs", "ok", "seconds_median"], lines, text="run,check")
t.done()
