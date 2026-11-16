import statistics

import tables as t

# One run: the sum over its workers of bytes per ns (steps x size / ns, on each worker's own clock).
agg = lambda rs: sum(r["steps"] * r["size"] / r["ns"] for r in rs)
groups = t.grouped(t.rows("flow"), lambda r: (t.series(r), r["machines"], r["hosts"], r["size"], r["cap"]), agg)
for k, per_run in t.grouped([r for r in t.rows("osu") if r["unit"] == "MB/s"],
                            lambda r: (t.series(r), r["machines"], r["hosts"], r["size"], 1),
                            lambda rs: statistics.fmean(r["value"] for r in rs) / 1000.0).items():
    groups[k] = per_run  # MB/s to bytes per ns; osu_bw's window is not the capacity, recorded as 1
t.table("bandwidth-size", ["series", "machines", "hosts", "size", "cap"], groups, ["gbps_median", "gbps_min", "gbps_max"])
t.table("aggregate-hosts", ["series", "machines", "hosts"], {k[:3]: v for k, v in groups.items() if k[3] == 65536 and k[4] == 8},
        ["gbps_median", "gbps_min", "gbps_max"])
t.done()
