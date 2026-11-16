import tables as t

# One row per run name and mutant: how many programs ran, how many killed it. `expect` is the catalogue's.
by = {}
for r in t.rows("kill"):
    by.setdefault((r["alias"], r["mutant"], r["rule"], r["expect"]), []).append(r)
lines = []
for (alias, mutant, rule, expect), rs in by.items():
    skipped = all(r["how"] == "skipped" for r in rs)
    lines.append([alias, mutant, rule, expect, "skipped" if skipped else len(rs), sum(1 for r in rs if r["killed"]),
                  "skipped" if skipped else ("killed" if any(r["killed"] for r in rs) else "survived")])
t.write("mutants-kills", ["run", "mutant", "rule", "expect", "programs", "killed", "result"], lines, text="run,mutant,rule,expect,result")
t.done()
