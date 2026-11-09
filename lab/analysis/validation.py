"""VALIDATION of the suite (lab/suite.nix): the verdicts of nixsci's claim report as one table.

    validation.py CELLS CLAIMS_REPORT OUT.tsv

CELLS is the suite's own list (Nix wrote it): every latency cell (backend, placement, size, Q1 p50 or Q2 p99) of the
reference deployment with its bound in ns or null, and every programs check. CLAIMS_REPORT is nixsci's claims-report.json
(absent when there is no claim). One row per cell, columns: id, backend, check, placement, size, bound_ns, verdict,
estimate, lo, hi, runs. The verdict is nixsci's: supported, contradicted or inconclusive (a claim of the whole population
of runs at the report's confidence, one sample per run, no correction for several claims). A cell with no bound has
the verdict "no bound": nothing was claimed, and the round trips are in comparison's latency.tsv.

The exit status is 0 always: the report records verdicts, it does not gate the build."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def num(x) -> str:
    return "" if x is None else (f"{x:.6g}" if isinstance(x, float) else str(x))


def main(cells_path: str, report_path: str, out_path: str) -> None:
    cells = json.loads(Path(cells_path).read_text())
    report = json.loads(Path(report_path).read_text()) if Path(report_path).exists() else {"claims": {}}
    claims = report["claims"]
    header = ["id", "backend", "check", "placement", "size", "bound_ns", "verdict", "estimate", "lo", "hi", "runs"]
    lines = []

    def row(i: str, backend: str, check: str, placement: str, size, bound) -> None:
        entry = claims.get(i)
        if entry is None:
            lines.append([i, backend, check, placement, size, bound, "no bound" if check != "programs" else "no runs", None, None, None, None])
            return
        r = entry["original"]
        verdict = r["verdict"]
        lines.append([i, backend, check, placement, size, bound, verdict, r.get("estimate"), r.get("lo"), r.get("hi"), r.get("n")])

    for c in cells["cells"]:
        row(c["id"], c["backend"], "Q1 median" if c["key"] == "p50" else "Q2 p99", c["placement"], c["size"], c["bound"])
    for p in cells["programs"]:
        row(p["id"], p["backend"], "programs", f"{p['machines']} machines", "", None)
    Path(out_path).write_text("\t".join(header) + "\n" + "".join("\t".join(num(x) for x in l) + "\n" for l in sorted(lines, key=lambda l: l[0])))
    counts: dict[str, int] = {}
    for l in lines:
        counts[l[6]] = counts.get(l[6], 0) + 1
    print(f"validation ({cells['deployment']}): {counts}", file=sys.stderr)


if __name__ == "__main__":
    main(*sys.argv[1:4])
