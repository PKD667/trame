"""What the analyses of the experiments share: read the view, summarise over runs, write a table and its figure source.

An analysis (lab/<experiment>/analysis.py, or an experiment of a backend's lab) imports this module, and nixsci runs it in a
directory that holds this file, the analysis and `figures.json` (the experiment's `figures`, written by nix/lab.nix).
Every number is a statistic over runs: each run gives one value, and a table gives the median, the minimum and the
maximum over the runs and their count. A series is a backend running trame (`<backend>`) or a baseline (`<backend>/<impl>`).

`write(name, ...)` writes `tables/<name>.tsv` and `figures/<name>.tex`: a document (usi-design, usi-figures) that sets the
table with \\usitable. Build it from the output directory: `usi-latexmk figures/<name>.tex`; the analysis does not render.
`done()` is the contract: it fails unless the figures written are exactly the experiment's `figures`.
Environment: NIXSCI_VIEW (the compacted runs, one directory per run name), NIXSCI_OUT.
"""

from __future__ import annotations

import json
import os
import statistics
import sys
from collections import defaultdict
from pathlib import Path

import pyarrow.parquet as pq

VIEW = Path(os.environ["NIXSCI_VIEW"])
OUT = Path(os.environ["NIXSCI_OUT"])
_written: list[str] = []

def aliases() -> list[str]:
    return sorted(p.name for p in VIEW.iterdir() if p.is_dir())


def rows(dataset: str) -> list[dict]:
    """The rows of a dataset in every run name (alias) that has it, each with its `alias`."""
    result = []
    for alias in aliases():
        directory = VIEW / alias / dataset
        for f in sorted(directory.glob("*.parquet")) if directory.is_dir() else []:
            result += [{**r, "alias": alias} for r in pq.read_table(f).to_pylist()]
    return result


def series(r: dict) -> str:
    return r["backend"] if r["impl"] == "trame" else f"{r['backend']}/{r['impl']}"


def quantile(xs: list[float], q: float) -> float:
    """The q-quantile, nearest rank (no interpolation: a round trip is a measured value)."""
    xs = sorted(xs)
    return xs[min(len(xs) - 1, max(0, int(q * len(xs) + 0.999999) - 1))]


def summary(per_run: dict[str, float]) -> tuple[int, float, float, float]:
    """(runs, median, min, max) of the value each run gave."""
    v = list(per_run.values())
    return len(v), statistics.median(v), min(v), max(v)


FIGURE = r"""\documentclass{usi-design}
\usepackage{usi-figures}
\begin{document}
\usitable[text={%s}]{tables/%s.tsv}
\end{document}
"""


def write(name: str, header: list[str], lines: list[list], text: str = "series") -> None:
    """The table `name` and the source of its figure. `text` names the columns that hold words, comma separated."""
    path = OUT / "tables" / f"{name}.tsv"
    path.parent.mkdir(parents=True, exist_ok=True)

    def cell(x) -> str:
        return "" if x is None else (f"{x:.6g}" if isinstance(x, float) else str(x))

    path.write_text("\t".join(header) + "\n" + "".join("\t".join(cell(x) for x in line) + "\n" for line in sorted(lines, key=lambda l: [str(x) for x in l])))
    figure = OUT / "figures" / f"{name}.tex"
    figure.parent.mkdir(parents=True, exist_ok=True)
    figure.write_text(FIGURE % (text, name))
    _written.append(name)


def done() -> None:
    """The contract: the figures written are the experiment's `figures` (figures.json beside this file), no more, no less."""
    want = sorted(json.loads((Path(__file__).parent / "figures.json").read_text()))
    got = sorted(_written)
    if want != got:
        sys.exit(f"figures: the experiment declares {want}, the analysis wrote {got}")


def grouped(data: list[dict], key, value) -> dict[tuple, dict[str, float]]:
    """{key: {run: value of the run}}: `value` maps the rows of one run under one key to a number (or None)."""
    runs: dict[tuple, dict[str, list[dict]]] = defaultdict(lambda: defaultdict(list))
    for r in data:
        runs[key(r)][r["run"]].append(r)
    result: dict[tuple, dict[str, float]] = {}
    for k, by_run in runs.items():
        vs = {run: value(rs) for run, rs in by_run.items()}
        result[k] = {run: v for run, v in vs.items() if v is not None}
    return {k: v for k, v in result.items() if v}


def table(name: str, head: list[str], groups: dict[tuple, dict[str, float]], unit_cols: list[str], text: str = "series") -> None:
    lines = []
    for k, per_run in groups.items():
        n, med, lo, hi = summary(per_run)
        lines.append([*k, n, med, lo, hi])
    write(name, [*head, "runs", *unit_cols], lines, text)
