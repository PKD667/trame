import statistics

import tables as t


def cpu_per_wall(rs):
    live = [r for r in rs if r["wall_s"] > 0]
    return statistics.fmean((r["user_s"] + r["sys_s"]) / r["wall_s"] for r in live) if live else None


data = [r for r in t.rows("cpu") if r["bench"] == "idle"]
groups = t.grouped(data, lambda r: (t.series(r), r["machines"], r["hosts"], r["per_host"]), cpu_per_wall)
t.table("idle-cpu", ["series", "machines", "hosts", "workers_per_host"], groups,
        ["cpu_per_wall_median", "cpu_per_wall_min", "cpu_per_wall_max"])
t.done()
