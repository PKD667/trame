#!/usr/bin/env python3
"""The guarantees map: what each backend checks of CONTRACT.md, and what it does not.

The rows are `src/backends/<backend>/guarantees.tsv`, one file per backend (clause, checked by, where, not
checked), written by hand and kept honest by this script. The check `guarantees` (nix/validate.nix) fails when:

  - a clause of CONTRACT.md has no row for a backend, or a row names a clause that is not in CONTRACT.md;
  - a cited path (`where`) does not exist in the tree the check reads;
  - a row's methods or lacks are empty (a clause with nothing lacking is a claim nobody audited);
  - the generated section of src/backends/README.md differs from the committed one (edit the tsv, then
    `check/guarantees.py map --root .` to regenerate and commit it).

A row is prose. The check cannot tell an honest "not checked" from a dishonest one; it keeps the inventory
from rotting, and drift between the tree and the words.

Usage:
  check/guarantees.py check --root DIR   verify; exit 1 and print why on any failure
  check/guarantees.py map  --root DIR   write the generated section into src/backends/README.md
"""

import argparse
import os
import re
import sys

CLAUSE = re.compile(r"^\*\*([A-Z]\d+) ([^*]+)\.\*\*")
MARK_START = "<!-- guarantees:start -->"
MARK_END = "<!-- guarantees:end -->"
COLS = ("clause", "checked_by", "where", "not_checked")


def clauses(root):
    """The clauses of CONTRACT.md, in its order: id and title."""
    out = []
    with open(os.path.join(root, "CONTRACT.md")) as f:
        for line in f:
            m = CLAUSE.match(line)
            if m:
                out.append((m.group(1), m.group(2).rstrip()))
    return out


def backend_dir(root):
    return os.path.join(root, "src", "backends")


def backends(root):
    """Every backend that the flake sees (a directory with backend.nix); each must carry guarantees.tsv."""
    bd = backend_dir(root)
    names = sorted(n for n in os.listdir(bd) if os.path.isfile(os.path.join(bd, n, "backend.nix")))
    missing = [n for n in names if not os.path.isfile(os.path.join(bd, n, "guarantees.tsv"))]
    return names, missing


def rows(root, name, errors):
    """The rows of one backend's tsv, each CLAUSE-id, checked_by, where, not_checked."""
    path = os.path.join(backend_dir(root), name, "guarantees.tsv")
    out = []
    with open(path) as f:
        for i, line in enumerate(f, 1):
            line = line.rstrip("\n")
            if not line.strip():
                continue
            cols = line.split("\t")
            if len(cols) != len(COLS):
                errors.append(f"{name}/guarantees.tsv:{i}: {len(cols)} columns, want {len(COLS)} (tab-separated)")
                continue
            out.append(dict(zip(COLS, cols)))
    return out


def check_paths(root, name, row, errors):
    where = row["where"].split()
    if not where:
        errors.append(f"{name} {row['clause']}: no path cited in `where`")
    for p in where:
        if not os.path.lexists(os.path.join(root, p)):
            errors.append(f"{name} {row['clause']}: cited path not in the tree: {p}")


def check_rows(root, name, rows_, clause_ids, errors):
    ids = [r["clause"] for r in rows_]
    for cid in set(clause_ids) - set(ids):
        errors.append(f"{name}: no row for {cid}")
    for cid in sorted(set(ids) - set(clause_ids)):
        errors.append(f"{name}: row for {cid}, which CONTRACT.md does not state")
    for r in rows_:
        if not r["checked_by"].strip():
            errors.append(f"{name} {r['clause']}: `checked by` is empty")
        if not r["not_checked"].strip():
            errors.append(f"{name} {r['clause']}: `not checked` is empty: nothing lacking is a claim nobody audited")
        check_paths(root, name, r, errors)


def section(root, data):
    """The generated section of src/backends/README.md, from the rows of every backend."""
    names, _ = backends(root)
    titles = dict(clauses(root))
    out = [
        MARK_START,
        "",
        "## Guarantees by backend",
        "",
        f"Generated from `src/backends/<backend>/guarantees.tsv` (which also names the paths) by "
        f"`check/guarantees.py`; the verdict `guarantees` (nix/validate.nix) fails on drift, a missing row or a "
        f"cited path that is not in the tree.",
        "",
    ]
    for name in names:
        rows_ = data.get(name, [])
        out += [f"### {name}", "", "| Clause | Checked by | Not checked |", "| --- | --- | --- |"]
        for r in rows_:
            title = titles.get(r["clause"], "?")
            out.append(f"| **{r['clause']}** {title} | {r['checked_by']} | {r['not_checked']} |")
        out.append("")
    out.append(MARK_END)
    return "\n".join(out)


def read_readme(root):
    path = os.path.join(backend_dir(root), "README.md")
    text = open(path).read()
    return path, text


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["check", "map"])
    ap.add_argument("--root", default=".", help="the tree the rows cite paths in")
    args = ap.parse_args()
    errors = []
    names, missing = backends(args.root)
    for name in missing:
        errors.append(f"{name}: src/backends/{name}/guarantees.tsv is missing: a backend carries its own rows")
    clause_ids = [c for c, _ in clauses(args.root)]
    if not clause_ids:
        errors.append("CONTRACT.md states no clause: the extractor found nothing")
    data = {}
    # A backend with no tsv is an error already; its rows cannot be read.
    for name in [n for n in names if n not in missing]:
        data[name] = rows(args.root, name, errors)
        check_rows(args.root, name, data[name], clause_ids, errors)

    readme_path, text = read_readme(args.root)
    if MARK_START not in text or MARK_END not in text:
        errors.append(f"src/backends/README.md has no guarantees section between {MARK_START} and {MARK_END}")
    else:
        start = text.index(MARK_START)
        end = text.index(MARK_END) + len(MARK_END)
        committed = text[start:end]
        generated = section(args.root, data)
        if args.mode == "map":
            open(readme_path, "w").write(text[:start] + generated + text[end:])
            print(f"wrote the section into {readme_path}")
        elif committed != generated:
            errors.append(
                "src/backends/README.md: the committed section differs from the one generated from the rows; "
                "run check/guarantees.py map --root . and commit it"
            )
    for e in errors:
        print(e)
    if errors:
        sys.exit(1)
    if args.mode == "check":
        print(f"ok: {len(clause_ids)} clauses x {len(names)} backends; rows and paths verified; map in sync")


if __name__ == "__main__":
    main()
