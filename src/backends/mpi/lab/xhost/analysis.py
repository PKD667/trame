import tables as t

# One row per mode, phase and frame size: the median and the 99th percentile of the one-way time, over runs.
data = t.rows("latency")
key = lambda r: (r["alias"], r["mode"], r["phase"], r["size"])
p50 = t.grouped(data, key, lambda rs: rs[0]["median_ns"])
p99 = t.grouped(data, key, lambda rs: rs[0]["p99_ns"])
lines = []
for k, per_run in p50.items():
    n, med, lo, hi = t.summary(per_run)
    lines.append([*k, n, med, lo, hi, t.summary(p99[k])[1]])
t.write("xhost-latency", ["run", "mode", "phase", "size", "runs", "median_ns", "median_min", "median_max", "p99_ns"], lines,
        text="run,mode,phase")
t.done()
