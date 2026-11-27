#!/usr/bin/env python3
"""The guarantee rows: what each backend checks of CONTRACT.md, and what it does not.

The rows are `src/backends/<backend>/guarantees.tsv`, one file per backend (clause, checked by, where, not
checked), written by hand and kept honest by this script. The check `guarantees` (nix/validate.nix) fails when:

  - a clause of CONTRACT.md has no row for a backend, or a row names a clause that is not in CONTRACT.md;
  - a cited path (`where`) does not exist in the tree the check reads;
  - a row's methods or lacks are empty (a clause with nothing lacking is a claim nobody audited);
The report a reader sees is built by the CI from these rows and the locked runs (`check/report.py`); nothing
generated is committed.

A row is prose. The check cannot tell an honest "not checked" from a dishonest one; it keeps the inventory
from rotting, and drift between the tree and the words.

The `checked by` cell is a comma-separated list over a closed vocabulary: `check/vocabulary.tsv` holds the
canonical method tokens, and every token of a row is refused that is neither one nor an alias below nor a
same-backend reference (`as S3`). The certificate (`check/certify.py`) reads the same vocabulary and resolves
the same cells, so no row can mean one thing to the check and another to the certificate.

Usage:
  check/guarantees.py check --root DIR   verify; exit 1 and print why on any failure
"""

import argparse
import os
import re
import sys

CLAUSE = re.compile(r"^\*\*([A-Z]\d+) ([^*]+)\.\*\*")
MARK_START = "<!-- guarantees:start -->"
MARK_END = "<!-- guarantees:end -->"
COLS = ("clause", "checked_by", "where", "not_checked")
VOCAB = "check/vocabulary.tsv"  # read from the tree --root points at, so the check and the tree agree
AS_CLAUSE = re.compile(r"^as ([A-Z]\d+)$")

# The phrases the rows write for a canonical method token. An alias maps a phrase to the token that names
# the same method, each grounded in the file the row cites; a phrase absent here is an error, not a guess:
# add the alias with its ground, or write the canonical token in the row.
ALIASES = {
    # the rows spell these three with spaces or the singular:
    "by construction": "by-construction",
    "model checking": "model-checking",
    "outcome check": "outcome-check",
    "program": "programs",
    # a suite the row names by what it tests is the token `tests`:
    "transport tests": "tests",     # the transport's own test binary, cited by the row that has one
    "launch tests": "tests",         # src/backends/<backend>/tests_launch.rs, the launch cases
    "unit test": "tests",            # M8's unit test of the share reading
    "no-alloc test": "tests",        # tests/no_alloc.rs (L3)
    "allocation test": "tests",      # tests/alloc.rs (M4)
    # phrases that name the construction the claim rests on (each names the code the row cites):
    "collective refusal": "by-construction",  # L1: open.rs, every collective reached on every rank
    "agreement at open": "by-construction",    # L2: open.rs, one plan over the world communicator
    "plan built once": "by-construction",       # L3: adopt.rs builds once, every rank adopts
    "connection check": "by-construction",     # F3: connect.rs compares the hosts' port declarations
    "stuck handling": "by-construction",        # E2: stuck.rs decides stuckness in construction
    "exact": "replay",                           # S2: conformance replays choices where the backend is exact
    "vacuous": "by-construction",               # F5 on one host: no lanes exist to keep the clause of
    "supply reading": "by-construction",         # M8: system.rs computes the share; programs test the arithmetic
}


def vocabulary(root):
    """The closed vocabularies of check/vocabulary.tsv: {clause, method, status: [tokens in file order]}.
    A line outside the three sections is an error: the file is the one source both checks read."""
    path = os.path.join(root, VOCAB)
    if not os.path.isfile(path):
        raise SystemExit(f"{VOCAB} is missing from the tree: the vocabulary is the certificate's source")
    out = {"clause": [], "method": [], "status": []}
    for i, line in enumerate(open(path), 1):
        line = line.rstrip("\n")
        if not line.strip() or line.startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) != 2 or cols[0] not in out or not cols[1].strip():
            raise SystemExit(f"{VOCAB}:{i}: want '<section><TAB><token>' with section in {sorted(out)}")
        out[cols[0]].append(cols[1].strip())
    return out


def canonical_methods(cell, clause_ids, errors, where):
    """The canonical method tokens of one `checked by` cell; every token must resolve or the row fails.
    `as S3` resolves to nothing here (it is valid when S3 is a clause); check/certify.py expands it."""
    out = []
    for token in (t.strip() for t in cell.split(",")):
        if not token:
            continue
        m = AS_CLAUSE.match(token)
        if m:
            if m.group(1) not in clause_ids:
                errors.append(f"{where}: 'as {m.group(1)}' names no clause of CONTRACT.md")
            continue
        canon = ALIASES.get(token, token if token in VOCAB_CACHE["method"] else None)
        if canon is None:
            errors.append(
                f"{where}: '{token}' is not a canonical method token of {VOCAB} nor one of its aliases"
            )
        elif canon not in out:
            out.append(canon)
    return out

VOCAB_CACHE = {"method": []}  # set by main once; canonical_methods reads it


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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".", help="the tree the rows cite paths in")
    args = ap.parse_args()
    errors = []
    vocab = vocabulary(args.root)
    VOCAB_CACHE["method"] = vocab["method"]
    # The clause section is extracted from CONTRACT.md and never retyped: it must be exactly its clauses.
    contract_ids = [c for c, _ in clauses(args.root)]
    if list(vocab["clause"]) != contract_ids:
        errors.append(
            f"{VOCAB}: the clause section is not CONTRACT.md's clause list; regenerate it from the file"
        )
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
        for r in data[name]:
            canonical_methods(r["checked_by"], clause_ids, errors, f"{name} {r['clause']}")

    for e in errors:
        print(e)
    if errors:
        sys.exit(1)
    print(f"ok: {len(clause_ids)} clauses x {len(names)} backends; rows and paths verified; vocabulary ok")


if __name__ == "__main__":
    main()
