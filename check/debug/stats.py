#!/usr/bin/env python3
"""Summaries of a directory of trame debug dumps: per op, per agent, longest WAIT intervals,
SEND_FULL counts, PEER_* gaps, per-worker step throughput (docs/debug.md).

What "worker" is here: a record's worker is its (rank, agent) pair. On a one-process backend (seq)
agent 0 is the leader's main thread and agent k+1 is worker k. Where a host is several processes, a
worker is a rank, whose agents are its threads. Both readings are the same pair of columns.

Usage:
  check/debug/stats.py DIR [--top N]

  --top   how many rows the interval and gap sections print (default 10)

Stdlib only, like check/guarantees.py.
"""

from __future__ import annotations

import argparse
import sys
import os
from collections import Counter, defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import read  # noqa: E402  (the suite is run as a script; its directory holds read.py)


def ms(ns):
    """Nanoseconds as milliseconds, kept readable."""
    return f"{ns / 1_000_000:.3f}"


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], epilog="docs/debug.md")
    p.add_argument("dir", help="a directory of rank*.dump files")
    p.add_argument("--top", type=int, default=10, help="rows per interval/gap section (default 10)")
    a = p.parse_args(argv)

    dumps = read.read_dir(a.dir)
    if not dumps:
        return p.error(f"{a.dir}: no rank*.dump files")

    # All records in file order, and per (rank, agent) in time order.
    per_op = Counter()
    per_agent = Counter()
    per_worker = defaultdict(list)
    for h, records in dumps:
        print(f"file {os.path.basename(h.path)}: rank {h.rank} host {h.host} {h.count} record(s), "
              f"arena {h.arena} bytes, t0 {h.t0}")
        for r in records:
            per_op[read.op_name(r.op)] += 1
            per_agent[(r.rank, r.host, r.agent)] += 1
            per_worker[(r.rank, r.agent)].append(r)

    print("\nper op:")
    for name, n in sorted(per_op.items()):
        print(f"  {name:<10} {n:>10}")

    print("\nper agent (rank, host, agent): records")
    for (rank, host, agent), n in sorted(per_agent.items()):
        print(f"  rank {rank} host {host} agent {agent}: {n:>10}")

    # The WAIT intervals: a WAIT to the same agent's next record (a wake, a step end, anything).
    waits = []
    for (rank, agent), rs in per_worker.items():
        for i, r in enumerate(rs):
            if r.op == read.NAMES["WAIT"] and i + 1 < len(rs):
                waits.append((rs[i + 1].t - r.t, rank, agent, r.t, rs[i + 1].op))
    waits.sort(reverse=True)
    print(f"\nlongest WAIT intervals (to the same agent's next record): {len(waits)} in total")
    for gap, rank, agent, t, nxt in waits[: a.top]:
        print(f"  {ms(gap)} ms  rank {rank} agent {agent}  waited at t+{ms(t)} ms, next op {read.op_name(nxt)}")

    fulls = Counter((r.rank, r.a, r.b) for rs in per_worker.values() for r in rs if r.op == read.NAMES["SEND_FULL"])
    print(f"\nSEND_FULL counts: {sum(fulls.values())} in total")
    for (rank, tag, route), n in sorted(fulls.items()):
        print(f"  rank {rank} tag {tag:#018x} route {route}: {n}")

    # The PEER_* gaps: between consecutive peer events of one rank (leader), in time order.
    peer = defaultdict(list)
    for h, records in dumps:
        for r in records:
            if r.op in (read.NAMES["PEER_SEND"], read.NAMES["PEER_DRAIN"], read.NAMES["PEER_WAIT"]):
                peer[r.rank].append(r)
    gaps = []
    for rank, rs in peer.items():
        rs.sort(key=lambda r: r.t)
        for r, s in zip(rs, rs[1:]):
            gaps.append((s.t - r.t, rank, read.op_name(r.op), read.op_name(s.op), r.t))
    gaps.sort(reverse=True)
    print(f"\nlongest PEER_* gaps (between consecutive peer events of a rank): {len(gaps)} in total")
    for gap, rank, first, second, t in gaps[: a.top]:
        print(f"  {ms(gap)} ms  rank {rank}  {first} at t+{ms(t)} ms -> next {second}")

    print("\nper-worker step throughput (steps of one (rank, agent)):")
    for (rank, agent), rs in sorted(per_worker.items()):
        ends = [r for r in rs if r.op == read.NAMES["STEP_END"]]
        starts = [r for r in rs if r.op == read.NAMES["STEP_START"]]
        if not ends or not starts:
            continue
        span = max(r.t for r in ends) - min(r.t for r in starts)
        rate = (len(ends) / span * 1_000_000_000) if span else float("inf")
        print(f"  rank {rank} agent {agent}: {len(ends)} steps in {ms(span)} ms "
              f"({rate:.0f} steps/s, {ms(span / max(len(ends), 1))} ms/step)")


if __name__ == "__main__":
    sys.exit(main())
