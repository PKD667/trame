import statistics

import tables as t

ping = [r for r in t.rows("ping") if not r["warm"]]
keys = lambda r: (t.series(r), r["placement"], r["size"], r["max_gap_ns"])
p50 = t.grouped(ping, keys, lambda rs: t.quantile([r["ns"] for r in rs], 0.5))
p99 = t.grouped(ping, keys, lambda rs: t.quantile([r["ns"] for r in rs], 0.99))
bound = {keys(r): (r["bound_p50"], r["bound_p99"]) for r in ping}  # null when the deployment sets none (q.nix)
lines = []
for k, per_run in p50.items():
    n, med, lo, hi = t.summary(per_run)
    _, tail, tlo, thi = t.summary(p99[k])
    lines.append([*k, n, med, lo, hi, tail, tlo, thi, *bound[k]])
# The OSU rows are an average of one-way times: twice that is a round trip's mean, not its median; no tail, no bound.
osu = [r for r in t.rows("osu") if r["unit"] == "us"]
for k, per_run in t.grouped(osu, lambda r: (t.series(r), "same-host" if r["machines"] == 1 else "cross-host", r["size"], 0),
                            lambda rs: 2000.0 * statistics.fmean(r["value"] for r in rs)).items():
    n, med, lo, hi = t.summary(per_run)
    lines.append([*k, n, med, lo, hi, None, None, None, None, None])
t.write("latency", ["series", "placement", "size", "max_gap_ns", "runs", "p50_ns", "p50_min", "p50_max", "p99_ns", "p99_min", "p99_max",
                    "bound_p50_ns", "bound_p99_ns"], lines, text="series,placement")
t.done()
