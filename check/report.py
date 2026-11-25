#!/usr/bin/env python3
"""The CI report: what each backend certifies, and how it measures — built at every CI run, committed nowhere.

Reads the guarantee rows (`src/backends/<b>/guarantees.tsv`), the clauses of CONTRACT.md, and the locked runs the CI
hands it, and writes `report.md` plus the SVG figures it references into `--out`. The report says what it does not
know: a clause without a row, a backend without locked runs, a figure with no data are all printed, never skipped.

A figure is black on white, two significant figures, and says in its caption what it is measured from (run names,
commit, machine). No LaTeX, no raster: an SVG file beside the markdown.

Usage:
  check/report.py --root DIR --out DIR --commit REV [--run NAME=PATH]...
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
from pathlib import Path

CLAUSE = __import__("re").compile(r"^\*\*([A-Z]\d+) ([^*]+)\.\*\*")


def clauses(root: Path) -> list[tuple[str, str]]:
    out = []
    for line in (root / "CONTRACT.md").read_text().splitlines():
        m = CLAUSE.match(line)
        if m:
            out.append((m.group(1), m.group(2).rstrip()))
    return out


def rows(root: Path) -> dict[str, list[dict[str, str]]]:
    """The rows of every backend that carries them: {backend: [{clause, checked_by, where, not_checked}]}."""
    out = {}
    bd = root / "src" / "backends"
    for d in sorted(p for p in bd.iterdir() if (p / "backend.nix").is_file()):
        f = d / "guarantees.tsv"
        if not f.is_file():
            continue
        out[d.name] = [
            dict(zip(("clause", "checked_by", "where", "not_checked"), line.split("\t")))
            for line in f.read_text().splitlines()
            if line.strip()
        ]
    return out


def table(dirs: list[Path], name: str) -> list[dict[str, str]]:
    """The rows of nixsci's table `name`, from every analysis output that holds one. The analyses reduce the view;
    this file only formats what they derived, so a number here is the number a claim is judged on."""
    out = []
    for d in dirs:
        f = d / "tables" / f"{name}.tsv"
        if not f.is_file():
            continue
        lines = f.read_text().splitlines()
        head = lines[0].split("\t")
        out += [dict(zip(head, l.split("\t"))) for l in lines[1:] if l.strip()]
    return out


def number(row: dict[str, str], key: str) -> float | None:
    v = row.get(key, "")
    return float(v) if v not in ("", "null", "None") else None


def latency(dirs: list[Path], backend: str) -> dict[tuple[str, int, str], dict]:
    """{(placement, size, series): {runs, p50, p99, lo, hi}} for one backend, from the latency analysis's table."""
    out = {}
    for r in table(dirs, "latency"):
        series = r["series"]
        if series != backend and not series.startswith(f"{backend}/"):
            continue
        p50, lo, hi = number(r, "p50_ns"), number(r, "p50_min"), number(r, "p50_max")
        if p50 is None:
            continue
        out[(r["placement"], int(r["size"]), series)] = {
            "runs": int(r["runs"]),
            "p50": p50,
            "p99": number(r, "p99_ns") or 0.0,
            "lo": lo if lo is not None else p50,
            "hi": hi if hi is not None else p50,
        }
    return out


def sig2(x: float) -> str:
    """Two significant figures, as plain digits (a chart never prints 2.4e+02)."""
    if x == 0:
        return "0"
    places = 1 - int(__import__("math").floor(__import__("math").log10(abs(x))))
    r = round(x, places)
    return f"{r:.{max(0, places)}f}" if places >= 0 else f"{r:.0f}"


def human(ns: float) -> str:
    """A duration to two significant figures, the unit after the number."""
    for scale, unit in ((1e6, "ms"), (1e3, "µs")):
        if ns >= scale:
            return f"{sig2(ns / scale)} {unit}"
    return f"{sig2(ns)} ns"


def bytes_(n: int) -> str:
    return f"{n // 1024} KiB" if n >= 1024 else f"{n} B"


def svg_latency(series: dict[tuple[str, int, str], dict], path: Path) -> None:
    """A dot plot: one row per (placement, size), the median with the min-max over runs as an interval."""
    keys = sorted(series, key=lambda k: (k[0], k[1]))
    if not keys:
        return
    row_h, left, right, top, axis = 26, 190, 90, 26, 34
    width, height = 720, top + row_h * len(keys) + axis - 8
    hi = max(series[k]["hi"] for k in keys)
    x = lambda v: left + (width - left - right) * (v / hi if hi else 0)
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        f'<rect width="{width}" height="{height}" fill="#fff"/>',
        f'<g font-family="monospace" font-size="11" fill="#000" stroke="none">',
    ]
    for i, k in enumerate(keys):
        y = top + row_h * i + row_h // 2
        g = series[k]
        parts.append(f'<text x="8" y="{y + 4}">{k[0]}, {bytes_(k[1])}, {k[2]}</text>')
        parts.append(f'<line x1="{x(g["lo"]):.1f}" y1="{y}" x2="{x(g["hi"]):.1f}" y2="{y}" stroke="#000" stroke-width="1"/>')
        for v in (g["lo"], g["hi"]):
            parts.append(f'<line x1="{x(v):.1f}" y1="{y - 4}" x2="{x(v):.1f}" y2="{y + 4}" stroke="#000" stroke-width="1"/>')
        parts.append(f'<circle cx="{x(g["p50"]):.1f}" cy="{y}" r="3" fill="#fff" stroke="#000" stroke-width="1.5"/>')
        parts.append(f'<text x="{x(g["hi"]) + 8:.1f}" y="{y + 4}">{human(g["p50"])}</text>')
    axis_y = top + row_h * len(keys)
    for frac, anchor in ((0.0, "start"), (0.5, "middle"), (1.0, "end")):
        v = hi * frac
        parts.append(f'<line x1="{x(v):.1f}" y1="{axis_y}" x2="{x(v):.1f}" y2="{axis_y + 6}" stroke="#000" stroke-width="1.5"/>')
        parts.append(f'<text x="{x(v):.1f}" y="{axis_y + 20}" text-anchor="{anchor}">{human(v)}</text>')
    parts.append(f'<text x="{width - right}" y="{axis_y + 20}" text-anchor="end"></text>')
    parts += ["</g>", "</svg>"]
    path.write_text("\n".join(parts) + "\n")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--out", required=True)
    ap.add_argument("--commit", default="unknown")
    ap.add_argument("--tables", action="append", default=[], help="a nixsci analysis output directory")
    args = ap.parse_args()
    root, out = Path(args.root), Path(args.out)
    (out / "figures").mkdir(parents=True, exist_ok=True)
    dirs = [Path(d) for d in args.tables]

    cs = clauses(root)
    per_backend = rows(root)


    md = ["# trame, checked", ""]
    md += [
        f"Built by the CI at `{args.commit}` from `CONTRACT.md`, `src/backends/<backend>/guarantees.tsv` and the "
        "nixsci analyses of the locked runs. Generated, not committed: a missing row, a backend with no runs or a "
        "figure with no data is printed as such.",
        "",
        "## What each backend certifies",
        "",
        "Each row is a clause of the contract; the cell is what checks it on that backend, and below the table is",
        "what that check does not reach.",
        "",
    ]
    names = sorted(per_backend)
    md.append("| Clause | " + " | ".join(names) + " |")
    md.append("| --- | " + " | ".join("---" for _ in names) + " |")
    for cid, title in cs:
        cells = []
        for n in names:
            r = next((r for r in per_backend[n] if r["clause"] == cid), None)
            cells.append(r["checked_by"] if r else "**no row**")
        md.append(f"| **{cid}** {title} | " + " | ".join(cells) + " |")
    md += ["", "### What the checks do not reach", ""]
    for n in names:
        lim = [f"`{r['clause']}` {r['not_checked']}" for r in per_backend[n] if r["not_checked"].strip()]
        md += [f"**{n}**", ""] + [f"- {l}" for l in lim] + [""]

    md += ["## How it measures", ""]
    for n in names:
        mine = latency(dirs, n)
        md += [f"### {n}", ""]
        if not mine:
            md += ["No latency rows for this backend: nothing measured, nothing claimed.", ""]
            continue
        md += [
            "| series | placement | size | runs | p50 | p99 |",
            "| --- | --- | --- | --- | --- | --- |",
        ]
        for k in sorted(mine, key=lambda k: (k[2], k[0], k[1])):
            g = mine[k]
            tail = human(g["p99"]) if g["p99"] else "—"
            md.append(f"| {k[2]} | {k[0]} | {bytes_(k[1])} | {g['runs']} | {human(g['p50'])} | {tail} |")
        svg = out / "figures" / f"latency-{n}.svg"
        svg_latency({k: v for k, v in mine.items() if k[2] == n}, svg)
        md += ["", f"![round trip, {n}](figures/latency-{n}.svg)", "",
               "*Measured.* One dot per placement and size, at the median of the per-run medians; the bar is the",
               "minimum and maximum of those medians over the runs. From nixsci's latency table "
               f"(`lab/latency/analysis.py`) at `{args.commit}`.", ""]
    (out / "report.md").write_text("\n".join(md))
    print(f"wrote {out / 'report.md'} ({len(cs)} clauses, {len(names)} backends, {len(dirs)} analyses)")


if __name__ == "__main__":
    main()
