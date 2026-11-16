import tables as t

data = t.rows("bench")
key = lambda r: (r["machine"], r["table"], r["case"], r["dist"], r["slots"], r["variant"], r["items"])
t.table("simd-throughput", ["machine", "table", "case", "dist", "slots", "variant", "items"],
        t.grouped(data, key, lambda rs: rs[0]["median_mitems_s"]), ["mitems_s_median", "mitems_s_min", "mitems_s_max"],
        text="machine,table,case,dist,variant")

# Speedup of a lane variant over the scalar reference at 1e6 items, one ratio per run (both measured in that run).
at = {}
for r in data:
    if r["table"] == "bench" and r["items"] == 1_000_000:
        at.setdefault((r["run"], r["machine"], r["case"], r["dist"], r["slots"]), {})[r["variant"]] = r["median_mitems_s"]
ratios = {}
for (run, machine, case, dist, slots), variants in at.items():
    scalar = variants.get("scalar")
    for variant, v in variants.items():
        if variant != "scalar" and scalar:
            ratios.setdefault((machine, case, dist, variant), {})[run] = v / scalar
t.table("simd-speedup", ["machine", "case", "dist", "variant"], ratios, ["speedup_median", "speedup_min", "speedup_max"],
        text="machine,case,dist,variant")
t.done()
