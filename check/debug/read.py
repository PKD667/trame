#!/usr/bin/env python3
"""Read trame debug dumps (docs/debug.md): the record format as a module, and the reader.

A dump file is a 52-byte header followed by fixed 32-byte records, all little-endian:

  magic "TRAMEDMP" (8 bytes) | format version u32 = 1 | host u16 | rank u16 | launch id u64 |
  arena size u64 | record size u32 = 32 | count u64 | t0 wall clock u64 (ns since the epoch)

Each record is one event:

  t u64 (ns since the process's launch) | a u64 | b u64 | rank u32 | host u16 | agent u8 | op u8

`t0` aligns files approximately: their wall clocks are one kernel clock within a host, and cross-host
alignment is as good as the hosts' clock sync. The op table below says what `a` and `b` mean per op.

Usage as a module (this file has no CLI):

  import read
  header, records = read.read_dump("path/to/rank0.dump")
  dumps = read.read_dir("path/to/out")   # every rank*.dump, sorted by (host, rank)
"""

from __future__ import annotations

import glob
import os
import struct
import sys
from collections import namedtuple

MAGIC = b"TRAMEDMP"
VERSION = 1
REC_SIZE = 32

HEADER = struct.Struct("<8sIHHQQIQQ")  # magic version host rank launch arena rec_size count t0
RECORD = struct.Struct("<QQQIHBB")  # t a b rank host agent op
HEADER_LEN = HEADER.size  # 52

# The op table: op -> (code, what a is, what b is). The codes are fixed (docs/debug.md).
OPS = {
    "STEP_START": (1, "step index", "0"),
    "STEP_END": (2, "step index", "status code (0 progress, 1 wait, 2 done, 3 error, 4 fresh)"),
    "SEND": (3, "message type tag (io::tag of the payload type)", "outgoing route id (worker_route numbering)"),
    "SEND_FULL": (4, "message type tag", "outgoing route id, as SEND"),
    "RECV_HIT": (5, "message type tag", "first incoming route id of the type (worker_incoming numbering)"),
    "RECV_MISS": (6, "message type tag", "0"),
    "WAIT": (7, "what: 0 wait(void), 1 room, 2 frame", "message type tag for a room, else 0"),
    "WAKE": (8, "epoch before the idle", "epoch after the wake"),
    "EDIT": (9, "target: 1 worker, 2 pool", "worker index, 0 for the pool"),
    "STOP": (10, "0", "0"),
    "JOIN": (11, "workers of the run", "0"),
    "PEER_SEND": (12, "host sent to", "peer message tag"),
    "PEER_DRAIN": (13, "messages moved into pending", "arrivals so far"),
    "PEER_WAIT": (14, "0", "0"),
    "SHARE": (15, "bytes shared", "0"),
    "RELAY_IN": (16, "lane's route id (the route's index in the topology)", "frame bytes"),
    "RELAY_OUT": (17, "lane's route id, as RELAY_IN", "frame bytes"),
    "EPOCH_BUMP": (18, "epoch word's byte offset in the window", "epoch value after the bump"),
    "DROP": (19, "records dropped after the arena filled", "0"),
    "MARK": (20, "user marker", "0"),
}
NAMES = {name: code for name, (code, _, _) in OPS.items()}
BY_CODE = {code: name for name, (code, _, _) in OPS.items()}

Header = namedtuple("Header", "version host rank launch arena record_size count t0 path")
Record = namedtuple("Record", "t a b rank host agent op path")


def op_name(code):
    """The op's name, or f"op{code}" for a code this reader does not know."""
    return BY_CODE.get(code, f"op{code}")


def read_dump(path):
    """One dump file: (Header, [Record]) in file order. ValueError when the file is not a dump."""
    with open(path, "rb") as f:
        blob = f.read()
    magic, version, host, rank, launch, arena, rec_size, count, t0 = HEADER.unpack_from(blob, 0)
    if magic != MAGIC:
        raise ValueError(f"{path}: not a trame debug dump (magic {magic!r})")
    if version != VERSION:
        raise ValueError(f"{path}: format version {version}, this reader reads {VERSION}")
    if rec_size != REC_SIZE:
        raise ValueError(f"{path}: record size {rec_size}, this reader reads {REC_SIZE}")
    header = Header(version, host, rank, launch, arena, rec_size, count, t0, str(path))
    records = []
    for i in range(count):
        t, a, b, r, h, agent, op = RECORD.unpack_from(blob, HEADER_LEN + i * REC_SIZE)
        records.append(Record(t, a, b, r, h, agent, op, str(path)))
    return header, records


def read_dir(directory):
    """Every rank*.dump of a directory: [(Header, [Record])], sorted by (host, rank)."""
    dumps = []
    for path in sorted(glob.glob(os.path.join(directory, "rank*.dump"))):
        try:
            dumps.append(read_dump(path))
        except ValueError as e:
            print(f"read: skipping {e}", file=sys.stderr)
    return sorted(dumps, key=lambda d: (d[0].host, d[0].rank))
