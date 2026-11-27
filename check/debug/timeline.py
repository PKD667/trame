#!/usr/bin/env python3
"""Print one merged chronological timeline from a directory of trame debug dumps (docs/debug.md).

The files are aligned on their headers' wall clock (t0), which is one kernel clock within a host and
as good as the hosts' clock sync across hosts (docs/debug.md, honesty). The printed t is nanoseconds
from the earliest aligned point in the directory, so it is comparable across files; --since and
--until filter on that same number.

Usage:
  check/debug/timeline.py DIR [--rank N ...] [--agent N ...] [--op NAME|N ...] [--since NS] [--until NS]

  --rank    only these ranks (the process, its rank in the launch)
  --agent   only these agent ids (threads of the process; 0 is the main thread)
  --op      only these ops, by name (SEND, RECV_HIT, ...) or by code
  --since   drop records before this t (ns, inclusive)
  --until   drop records at or after this t (ns)

Stdlib only, like check/guarantees.py.
"""

from __future__ import annotations

import argparse
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import read  # noqa: E402  (the suite is run as a script; its directory holds read.py)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], epilog="docs/debug.md")
    p.add_argument("dir", help="a directory of rank*.dump files")
    p.add_argument("--rank", type=int, action="append", help="only this rank (repeatable)")
    p.add_argument("--agent", type=int, action="append", help="only this agent id (repeatable)")
    p.add_argument("--op", action="append", help="only this op, by name or code (repeatable)")
    p.add_argument("--since", type=int, help="drop records before this t (ns)")
    p.add_argument("--until", type=int, help="drop records at or after this t (ns)")
    p.add_argument("--limit", type=int, default=0, help="print at most this many records (0: all)")
    a = p.parse_args(argv)

    ops = set()
    for name in a.op or []:
        ops.add(name.upper() if not name.isdigit() else int(name))

    dumps = read.read_dir(a.dir)
    if not dumps:
        return p.error(f"{a.dir}: no rank*.dump files")

    # Every record with its aligned wall clock (t0 + t) and the directory's zero point.
    zero = min(h.t0 for h, _ in dumps)
    rows = []
    for h, records in dumps:
        for r in records:
            wall = h.t0 + r.t
            if a.rank and r.rank not in a.rank:
                continue
            if a.agent and r.agent not in a.agent:
                continue
            if ops and (r.op not in ops and read.op_name(r.op) not in ops):
                continue
            t = wall - zero
            if a.since is not None and t < a.since:
                continue
            if a.until is not None and t >= a.until:
                continue
            rows.append((wall, r))
    rows.sort(key=lambda wr: wr[0])

    print(f"# timeline of {a.dir}: {len(rows)} record(s), t in ns from the earliest aligned point")
    print(f"{'t':>12}  {'rank':>4} {'host':>4} {'ag':>3}  {'op':<10} {'a':>18} {'b':>18}")
    for i, (wall, r) in enumerate(rows):
        if a.limit and i >= a.limit:
            print(f"... {len(rows) - i} more")
            break
        print(f"{wall - zero:>12}  {r.rank:>4} {r.host:>4} {r.agent:>3}  {read.op_name(r.op):<10} {r.a:>18} {r.b:>18}")


if __name__ == "__main__":
    sys.exit(main())
