"""COMPARISON of the suite (lab/suite.nix): one table per metric, the backends side by side. No verdicts: this part of the
report says what was measured, not whether it is good enough (the claims are validation.tsv's).

A series is a backend running trame (`<backend>`) or a plain baseline (`<backend>/plain` for the backend's own plain programs (the C ones), `<backend>/osu`
for the OSU micro-benchmarks, declared by src/backends/<backend>/experiments.nix). Every number is a statistic over runs:
each run gives one value, and the table gives the median, the minimum and the maximum over the runs and their count.
Experiments of one kind that share a parameter point (flow-bw and flow-agg both run size 65536, cap 8) pool their runs.

Tables, in $NIXSCI_OUT/tables/:
  latency.tsv            round trip: median and p99 of a run's trips (warm-up dropped), by series, placement, size, gap
  bandwidth_size.tsv     bandwidth by frame size and route capacity (aggregate over workers; bytes per ns = GB/s)
  aggregate_hosts.tsv    aggregate bandwidth by hosts, at size 65536 and capacity 8
  step_overhead.tsv      ns per call of a step that does nothing, by steps per worker and workers
  idle_cpu.tsv           CPU seconds per wall second of a rank that waits (GNU time), by workers
  round.tsv, fanin.tsv, graph.tsv   ns per round, per message, per step (and the compute share of a graph step)
figures.tex: the sources of the figures, on the tables above (usi-figures; build with usi-latexmk, the report does not).
"""

from __future__ import annotations

import os
import statistics
from collections import defaultdict
from pathlib import Path

import pyarrow.parquet as pq

from nixsci.lab.analysis import out

VIEW = Path(os.environ["NIXSCI_VIEW"])


def aliases() -> list[str]:
    return sorted(p.name for p in VIEW.iterdir() if p.is_dir())


def rows(dataset: str) -> list[dict]:
    """The rows of a dataset in every alias that has it."""
    result = []
    for alias in aliases():
        directory = VIEW / alias / dataset
        for f in sorted(directory.glob("*.parquet")) if directory.is_dir() else []:
            result += pq.read_table(f).to_pylist()
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


def write(name: str, header: list[str], lines: list[list]) -> None:
    path = out("tables", f"{name}.tsv")
    path.parent.mkdir(parents=True, exist_ok=True)

    def cell(x) -> str:
        return "" if x is None else (f"{x:.6g}" if isinstance(x, float) else str(x))

    path.write_text("\t".join(header) + "\n" + "".join("\t".join(cell(x) for x in line) + "\n" for line in sorted(lines, key=lambda l: [str(x) for x in l])))


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


def table(name: str, head: list[str], groups: dict[tuple, dict[str, float]], unit_cols: list[str]) -> None:
    lines = []
    for k, per_run in groups.items():
        n, med, lo, hi = summary(per_run)
        lines.append([*k, n, med, lo, hi])
    write(name, [*head, "runs", *unit_cols], lines)


def latency() -> None:
    ping = [r for r in rows("ping") if not r["warm"]]
    keys = lambda r: (series(r), r["placement"], r["size"], r["max_gap_ns"])
    p50 = grouped(ping, keys, lambda rs: quantile([r["ns"] for r in rs], 0.5))
    p99 = grouped(ping, keys, lambda rs: quantile([r["ns"] for r in rs], 0.99))
    lines = []
    for k, per_run in p50.items():
        n, med, lo, hi = summary(per_run)
        _, tail, tlo, thi = summary(p99[k])
        lines.append([*k, n, med, lo, hi, tail, tlo, thi])
    # The OSU rows are an average of one-way times: twice that is a round trip's mean, not its median; no tail.
    osu = [r for r in rows("osu") if r["unit"] == "us"]
    for k, per_run in grouped(osu, lambda r: (series(r), "same-host" if r["machines"] == 1 else "cross-host", r["size"], 0),
                              lambda rs: 2000.0 * statistics.fmean(r["value"] for r in rs)).items():
        n, med, lo, hi = summary(per_run)
        lines.append([*k, n, med, lo, hi, None, None, None])
    write("latency", ["series", "placement", "size", "max_gap_ns", "runs", "p50_ns", "p50_min", "p50_max", "p99_ns", "p99_min", "p99_max"],
          lines)


def flow() -> None:
    data = rows("flow")
    # One run: the sum over its workers of bytes per ns (steps x size / ns, on each worker's own clock).
    agg = lambda rs: sum(r["steps"] * r["size"] / r["ns"] for r in rs)
    groups = grouped(data, lambda r: (series(r), r["machines"], r["hosts"], r["size"], r["cap"]), agg)
    for k, per_run in grouped([r for r in rows("osu") if r["unit"] == "MB/s"],
                              lambda r: (series(r), r["machines"], r["hosts"], r["size"], 1),
                              lambda rs: statistics.fmean(r["value"] for r in rs) / 1000.0).items():
        groups[k] = per_run  # MB/s to bytes per ns; osu_bw's window is not the capacity, recorded as 1
    table("bandwidth_size", ["series", "machines", "hosts", "size", "cap"], groups, ["gbps_median", "gbps_min", "gbps_max"])
    table("aggregate_hosts", ["series", "machines", "hosts"],
          {k[:3]: v for k, v in groups.items() if k[3] == 65536 and k[4] == 8}, ["gbps_median", "gbps_min", "gbps_max"])


def step() -> None:
    data = rows("step")
    # ns / n is the time from one start of the step to the next; a run's value is the median over its steps.
    groups = grouped(data, lambda r: (series(r), r["machines"], r["hosts"], r["per"], r["k"]),
                     lambda rs: statistics.median(r["ns"] / r["n"] for r in rs))
    table("step_overhead", ["series", "machines", "hosts", "per", "k"], groups, ["ns_median", "ns_min", "ns_max"])


def idle() -> None:
    data = [r for r in rows("cpu") if r["bench"] == "idle"]
    groups = grouped(data, lambda r: (series(r), r["machines"], r["hosts"], r["per_host"]),
                     lambda rs: statistics.fmean((r["user_s"] + r["sys_s"]) / r["wall_s"] for r in rs if r["wall_s"] > 0) if any(r["wall_s"] > 0 for r in rs) else None)
    table("idle_cpu", ["series", "machines", "hosts", "workers_per_host"], groups, ["cpu_per_wall_median", "cpu_per_wall_min", "cpu_per_wall_max"])


def per_worker(bench: str, count: str, names: list[str]) -> None:
    data = rows(bench)
    groups = grouped(data, lambda r: (series(r), r["machines"], r["hosts"], r["workers"]),
                     lambda rs: statistics.median(r["ns"] / r[count] for r in rs if r.get("worker") is not None) if any(r.get("worker") is not None for r in rs) else None)
    table(bench, ["series", "machines", "hosts", "workers"], groups, names)


def graph() -> None:
    data = rows("graph")
    key = lambda r: (series(r), r["machines"], r["hosts"], r["workers"], r["size"])
    groups = grouped(data, key, lambda rs: statistics.median(r["ns"] / r["steps"] for r in rs))
    share = grouped(data, key, lambda rs: statistics.median(r["compute_ns"] / r["ns"] for r in rs))
    lines = []
    for k, per_run in groups.items():
        n, med, lo, hi = summary(per_run)
        lines.append([*k, n, med, lo, hi, summary(share[k])[1]])
    write("graph", ["series", "machines", "hosts", "workers", "size", "runs", "ns_per_step_median", "ns_per_step_min", "ns_per_step_max", "compute_share_median"], lines)


FIGURES = r"""% Figure sources for the suite report: \usitable on each table of tables/, \usidotplot on the two overheads. Build with
% usi-latexmk (nix run <agents>#usi-latexmk -- figures.tex); the report derivation does not render them.
\documentclass{usi-design}
\usepackage{usi-figures}
\begin{document}
\section*{Suite report: comparison}
%s\end{document}
"""


def figures() -> None:
    names = sorted(p.stem for p in out("tables").glob("*.tsv"))
    body = ""
    for n in names:
        text = "series,placement" if n == "latency" else "series"
        body += f"\\subsection*{{{n.replace('_', ' ')}}}\n\\usitable[text={{{text}}}]{{tables/{n}.tsv}}\n\n"
    out("figures.tex").write_text(FIGURES % body)


def main() -> None:
    latency()
    flow()
    step()
    idle()
    per_worker("round", "rounds", ["ns_per_round_median", "ns_per_round_min", "ns_per_round_max"])
    per_worker("fanin", "msgs", ["ns_per_msg_median", "ns_per_msg_min", "ns_per_msg_max"])
    graph()
    figures()


if __name__ == "__main__":
    main()
