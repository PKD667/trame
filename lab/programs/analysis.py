import statistics

import tables as t

# One row per run name and program: the verdicts of its runs. A run name is one backend on one number of machines.
by = {}
for r in t.rows("run"):
    by.setdefault((r["alias"], r["program"]), []).append(r)
lines = []
for (alias, program), rs in by.items():
    count = lambda v: sum(1 for r in rs if r["verdict"] == v)
    lines.append([alias, program, len(rs), count("ok"), count("diff"), count("exit"), count("timeout"),
                  statistics.median(r["seconds"] for r in rs), len({r["stdout_sha256"] for r in rs})])
t.write("programs-verdicts", ["run", "program", "runs", "ok", "diff", "exit", "timeout", "seconds_median", "distinct_stdout"], lines,
        text="run,program")
t.done()
