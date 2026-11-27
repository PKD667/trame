#!/usr/bin/env python3
"""The certificate: what each backend certifies of CONTRACT.md — program behaviour and memory safety.

Writes `certificate.tsv` and `certificate.md` into --out, one row per (backend, clause), built at CI time and
committed nowhere. A row is `generated` — from the guarantee rows (`src/backends/<backend>/guarantees.tsv`),
the model inventory (`<backend>/proofs/run.sh`), the latency bounds (`lab/latency/q.nix`), the locked runs
(`lab.lock`), the unsafe blocks of `src/` (the reading of `check/proofs/unsafe-registry.sh`, which is the
gate, not this file) and the counts of programs, rejection cases, tests and mutants — or `authored`: a row of
`src/backends/<backend>/certificates.tsv`, written after reading the code, at most three per backend.

Every computed cell says what it counted. Every row's `bound` says what the row's methods do not reach; an
empty bound is an error, not a warning. `status` is the strongest thing the methods establish, of the closed
vocabulary of `check/vocabulary.tsv`; nothing that can fail gives more than `unproven`.

Usage:
  check/certify.py --root DIR --out DIR
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import guarantees  # the same vocabulary, the same rows: one source, not two

COLS = ("backend", "clause", "claim", "method", "evidence", "bound", "status", "source")
AUTHORED_COLS = ("clause", "status", "claim", "evidence", "bound")
MEMORY = ("M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8")  # the memory clauses a SAFETY comment may name

# The model inventory: each model of the MODELS heredoc of a backend's proofs/run.sh maps to the clauses its
# module states. A model the heredoc adjudicates and this map does not name is an error: the map keeps up or fails.
MODEL_CLAUSES = {
    # Epoch.tla: the wait protocol of one epoch word, its sleepers and bumper (S3 Wake).
    "Epoch": ["S3"],
    # Epoch.tla at two sleepers (MCEpoch2): the same protocol at N >= 2.
    "Epoch2": ["S3"],
    # Epoch.tla at three sleepers (MCEpoch3).
    "Epoch3": ["S3"],
    # Epoch.tla with the transport lock XPORT (MCEpochXport): the worker rank as it runs.
    "EpochXport": ["S3"],
    # Epoch.tla, the earlier flag protocol (MCEpochFlag): NoLostWakeup is violated, a counterexample that must exist.
    "EpochFlag": ["S3"],
    # Epoch.tla, the flag protocol with the announced bumper (MCEpochFlagAnnounced): Announced is violated.
    "EpochFlagAnnounced": ["S3"],
    # Epoch.tla, the flag protocol under the transport lock (MCEpochFlagXport).
    "EpochFlagXport": ["S3"],
    # Epoch.tla at the wrap of the 32-bit word (MCEpochWrap): NoLostWakeup is violated.
    "EpochWrap": ["S3"],
    # Lane.tla: one lossless cross-host route of transport/cross, capacity by credit (F5 keeps T1-T3 while both workers run).
    "Lane": ["F5", "T1", "T2", "T3"],
    # Lane.tla with three frames in flight (MCLane3).
    "Lane3": ["F5", "T1", "T2", "T3"],
    # Lane.tla at the credit wrap (MCLaneOver): Chain is violated, a counterexample that must exist.
    "LaneOver": ["F5", "T1", "T2", "T3"],
    # Detect.tla: the leaders' deadlock detector of roles/detect.rs (E2: Deadlock only where the worker is stuck).
    "Detect": ["E2"],
    # Detect.tla, one round only (MCDetectOnce): Sound is violated.
    "DetectOnce": ["E2"],
    # EditNoWait.tla: the leader's edit handshake, whose module header names S1, S3, S4 and P2 (M5 it names as not modelled).
    "EditNoWait": ["S1", "S3", "S4", "P2"],
}

# A row's status, from its methods, strongest first. `model-checking` gives `proven-under-model` only when the
# model inventory covers the clause: a row may cite the method, the inventory says whether a model exists to
# back it. The rest: miri, the driver, replay, a witness, an outcome check or a rejection harness accepted a
# run or a program (`checked`); a measurement reported a number (`measured`, per deployment); programs, tests
# or the model oracle compared outputs with written expectations (`tested-only`); by-construction alone is an
# argument from reading, nothing that can fail (`unproven`).
STATUS_ORDER = ("proven-under-model", "checked", "measured", "tested-only", "unproven")
METHOD_STATUS = {
    "model-checking": "proven-under-model",
    "miri": "checked",
    "driver": "checked",
    "replay": "checked",
    "witness": "checked",
    "outcome-check": "checked",
    "rejections": "checked",
    "measurement": "measured",
    "programs": "tested-only",
    "tests": "tested-only",
    "model": "tested-only",
    "by-construction": "unproven",
}
# The reverse, for one purpose: an authored row has a status and no method column, and its status names the
# method class it adds (`unproven` adds by-construction: a reading). A generated row for the same clause that
# already has that method makes the authored row a duplicate, and it is refused.
STATUS_METHOD = {
    "proven-under-model": "model-checking",
    "checked": "witness",
    "tested-only": "programs",
    "measured": "measurement",
    "unproven": "by-construction",
}


def status_of(methods, covered):
    """The row's status and the limits the status itself does not reach (appended to the row's bound)."""
    notes = []
    m = list(methods)
    if "model-checking" in m and not covered:
        m.remove("model-checking")
        notes.append("the row cites model checking but no model of the inventory covers this clause")
    best = STATUS_ORDER[-1]
    for tok in m:
        cand = METHOD_STATUS[tok]
        if STATUS_ORDER.index(cand) < STATUS_ORDER.index(best):
            best = cand
    return best, notes


def resolve_methods(backend, row, vocab, errors):
    """The method tokens of a row's `checked by`: a `, `-separated list of canonical tokens of the
    vocabulary, the same cells check/guarantees.py validates; an empty cell, an empty token or a token
    outside the vocabulary is refused, so a row the check refuses cannot certify either."""
    out = []
    if not row["checked_by"].strip():
        errors.append(f"{backend} {row['clause']}: `checked by` is empty")
        return out
    for token in (t.strip() for t in row["checked_by"].split(",")):
        if not token:
            errors.append(f"{backend} {row['clause']}: `checked by` has an empty token")
        elif token not in vocab["method"]:
            errors.append(f"{backend} {row['clause']}: '{token}' is not a method token of the vocabulary")
        elif token not in out:
            out.append(token)
    return out


def inventory(root, backend, errors=None):
    """The models a backend's proofs/run.sh adjudicates, and the clauses each covers (MODEL_CLAUSES)."""
    run = os.path.join(root, "src", "backends", backend, "proofs", "run.sh")
    if not os.path.isfile(run):
        return {}
    models = []
    in_heredoc = False
    for line in open(run):
        s = line.strip()
        if s.endswith("<<'MODELS'"):
            in_heredoc = True
            continue
        if in_heredoc and s == "MODELS":
            break
        if in_heredoc and s:
            models.append(s.split(":")[0].strip())
    unknown = [m for m in models if m not in MODEL_CLAUSES]
    if unknown and errors is not None:
        errors.append(
            f"{backend}/proofs/run.sh adjudicates {', '.join(unknown)}, which MODEL_CLAUSES of check/certify.py does not map"
        )
    return {m: MODEL_CLAUSES[m] for m in models if m in MODEL_CLAUSES}


def q_bounds(root, errors):
    """The latency bounds of lab/latency/q.nix, read as text: the reference deployment, its sizes and
    placements, and the explicit entries {placement: {size: (p50, p99)}}. A bound set through the `unset`
    helper would not be seen, so the helper must stay null: a set bound appears as an explicit entry, or the
    reading refuses."""
    text = open(os.path.join(root, "lab", "latency", "q.nix")).read()
    ref = re.search(r'reference\s*=\s*"(\w+)"', text)
    sizes = re.search(r"sizes\s*=\s*\[([^\]]*)\]", text)
    placements = re.search(r'placements\s*=\s*\[([^\]]*)\]', text)
    helper = re.search(r'unset\s*=\s*\{\s*p50\s*=\s*(\S+?)\s*;\s*p99\s*=\s*(\S+?)\s*;', text)
    if not (ref and sizes and placements and helper):
        errors.append("lab/latency/q.nix: the reference deployment, sizes, placements or unset helper unreadable")
        return None
    if set(helper.groups()) != {"null"}:
        errors.append("lab/latency/q.nix: the unset helper sets a bound; certify.py reads explicit entries only")
    entry = re.compile(
        r'q\.(\w+)\.?"?([-\w]+)"?\.?"?(\d+)"?\s*=\s*\{\s*p50\s*=\s*(\d+|null)\s*;\s*p99\s*=\s*(\d+|null)\s*;'
    )
    bounds = {}
    for dep, placement, size, p50, p99 in entry.findall(text):
        if dep != ref.group(1):
            errors.append(f"lab/latency/q.nix: a bound on deployment {dep}, which is not the reference")
        bounds.setdefault(placement, {})[int(size)] = (p50, p99)
    return {
        "reference": ref.group(1),
        "sizes": [int(s) for s in re.findall(r"\d+", sizes.group(1))],
        "placements": re.findall(r'"([-\w]+)"', placements.group(1)),
        "bounds": bounds,
    }


def locked_runs(root):
    """lab.lock: {experiment: (runs locked, runs whose state is ok)}."""
    out = {}
    for k, v in json.load(open(os.path.join(root, "lab.lock")))["runs"].items():
        ok = sum(1 for r in v if r.get("facts", {}).get("state") == "ok")
        out[k] = (len(v), ok)
    return out


def counts(root, backend):
    """The counts a row's computed cells name. A cell that is computed says what it counted and where."""
    programs = len([f for f in os.listdir(os.path.join(root, "check", "programs")) if f.endswith(".rs")])
    rejections = sum(
        len([f for f in fs if f.endswith(".rs")])
        for _, _, fs in os.walk(os.path.join(root, "check", "rejections", "cases"))
    )
    tests = len([f for f in os.listdir(os.path.join(root, "tests")) if f.endswith(".rs")])
    mdir = os.path.join(root, "src", "backends", backend, "mutants")
    mutants = len([f for f in os.listdir(mdir) if f.endswith(".patch")]) if os.path.isdir(mdir) else 0
    return {"programs": programs, "rejections": rejections, "tests": tests, "mutants": mutants}


def unsafe_sites(root, backend):
    """Every `unsafe` site of the code a backend compiles: `src/` outside `src/backends` (shared, compiled for
    every backend) plus `src/backends/<backend>/`. Mirrors the walk of check/proofs/unsafe-registry.sh, whose
    gate this is not: directories named driver, rejections, tests, experiments, proofs and target are pruned
    at any depth, and so is every git submodule of .gitmodules. A site carries its SAFETY comment (directly
    above, attributes may sit between, or trailing) and the clause ids the comment names."""
    prune = {"driver", "rejections", "tests", "experiments", "proofs", "target", ".git"}
    modules = []
    gm = os.path.join(root, ".gitmodules")
    if os.path.isfile(gm):
        for line in open(gm):
            line = line.strip()
            if line.startswith("path"):
                modules.append(line.split("=", 1)[1].strip())
    tag = re.compile(r"\b([RLSTPMEQF]\d+)\b")
    keyword = re.compile(r"(^|[^A-Za-z0-9_])unsafe([^A-Za-z0-9_]|$)")

    def code(line):
        line = re.sub(r'"(?:[^"\\]|\\.)*"', '""', line)
        return re.sub(r"//.*", "", line)

    sites = []
    for dirpath, dirnames, filenames in os.walk(os.path.join(root, "src")):
        rel = os.path.relpath(dirpath, root)
        if any(rel == m or rel.startswith(m + os.sep) for m in modules):
            dirnames[:] = []
            continue
        dirnames[:] = sorted(d for d in dirnames if d not in prune)
        for fn in sorted(filenames):
            if not fn.endswith(".rs"):
                continue
            p = os.path.join(rel, fn)
            lines = open(os.path.join(root, p)).read().splitlines()
            for i, line in enumerate(lines, 1):
                if not keyword.search(code(line)):
                    continue
                text = ""
                if re.search(r"//.*(SAFETY:|# Safety)", line):
                    text = line
                j = i - 1
                while j >= 1 and re.match(r"^\s*#\[", lines[j - 1]):
                    j -= 1
                while j >= 1 and re.match(r"^\s*//", lines[j - 1]):
                    text = lines[j - 1] + " " + text
                    j -= 1
                sites.append((p, i, sorted(set(tag.findall(text)))))
    own = f"src{os.sep}backends{os.sep}{backend}"
    shared = [s for s in sites if not s[0].startswith(f"src{os.sep}backends")]
    backend_own = [s for s in sites if s[0].startswith(own + os.sep) or s[0] == own]
    return shared, backend_own


def miri_covers(root, backend):
    """Whether check/proofs/miri.sh runs this backend: it runs the tests of a backend whose descriptor has
    `cargoTests = true` (a backend that needs a launcher puts its tests outside cargo's reach)."""
    d = os.path.join(root, "src", "backends", backend, "backend.nix")
    return bool(os.path.isfile(d) and re.search(r"cargoTests\s*=\s*true\b", open(d).read()))


def authored_rows(root, backend, vocab, clause_ids, errors):
    """The hand-written rows of src/backends/<backend>/certificates.tsv, validated: clause and status from the
    vocabulary, every cited path in the tree, claim and bound non-empty, at most three rows, header exact."""
    path = os.path.join(root, "src", "backends", backend, "certificates.tsv")
    if not os.path.isfile(path):
        return []
    out = []
    for i, line in enumerate(open(path), 1):
        line = line.rstrip("\n")
        if not line.strip():
            continue
        cols = line.split("\t")
        where = f"{backend}/certificates.tsv:{i}"
        if i == 1:
            if cols != list(AUTHORED_COLS):
                errors.append(f"{where}: the header must be {' '.join(AUTHORED_COLS)}, tab-separated")
            continue
        if len(cols) != len(AUTHORED_COLS):
            errors.append(f"{where}: {len(cols)} columns, want {len(AUTHORED_COLS)} (tab-separated)")
            continue
        r = dict(zip(AUTHORED_COLS, cols))
        if r["clause"] not in clause_ids:
            errors.append(f"{where}: clause {r['clause']} is not in the vocabulary")
        if r["status"] not in vocab["status"]:
            errors.append(f"{where}: status '{r['status']}' is not a status token of the vocabulary")
        for col in ("claim", "bound"):
            if not r[col].strip():
                errors.append(f"{where}: {col} is empty")
        for tok in r["evidence"].split():
            p = tok.split(":")[0]
            if not os.path.lexists(os.path.join(root, p)):
                errors.append(f"{where}: cited path not in the tree: {p}")
        out.append(r)
    if len(out) > 3:
        errors.append(f"{backend}/certificates.tsv: {len(out)} rows, at most 3 are allowed")
    return out


def trim(s, n=72):
    """A phrase for the markdown: the tsv holds the whole cell, the page the phrase."""
    s = " ".join(s.split())
    return s if len(s) <= n else s[:n].rsplit(" ", 1)[0] + "…"


def generate(root, backend, vocab, clause_list, errors):
    """The generated rows of one backend: one per clause, in CONTRACT.md's order."""
    g_rows = guarantees.rows(root, backend, errors)
    by_clause = {r["clause"]: r for r in g_rows}
    models = inventory(root, backend, errors)
    covered = sorted({c for cs in models.values() for c in cs})
    q = q_bounds(root, errors)
    runs = locked_runs(root)
    cnt = counts(root, backend)
    shared, own = unsafe_sites(root, backend)
    all_sites = shared + own
    uncited = sum(1 for _, _, t in all_sites if not t)
    cited_any = len(all_sites) - uncited
    miri = miri_covers(root, backend)

    rows_ = []
    for cid, title in clause_list:
        r = by_clause.get(cid)
        if r is None:
            errors.append(f"{backend}: no guarantee row for {cid}: the certificate generates nothing")
            continue
        cov = cid in covered
        methods = resolve_methods(backend, r, vocab, errors)
        if cov and "model-checking" not in methods:
            methods.append("model-checking")
        if cid in MEMORY and miri:
            methods.append("miri")
        methods = [t for t in vocab["method"] if t in methods]  # the vocabulary's order, a stable table

        claim = f"{cid} {title}"
        evidence = [r["where"]]
        if "programs" in methods:
            evidence.append(f"check/programs: {cnt['programs']} programs (*.rs)")
            evidence.append(
                f"mutation: {cnt['mutants']} patches (src/backends/{backend}/mutants, verdict mutants-{backend})"
            )
        if "tests" in methods:
            evidence.append(f"tests: {cnt['tests']} test files (tests/*.rs)")
        if "rejections" in methods:
            evidence.append(f"check/rejections/cases: {cnt['rejections']} cases (*.rs, recursive)")
        if cov:
            names = ", ".join(sorted(m for m, cs in models.items() if cid in cs))
            evidence.append(f"models: {names} (proofs/run.sh; MODEL_CLAUSES of check/certify.py)")
        if cid in MEMORY:
            citing = [f"{p}:{l}" for p, l, t in all_sites if cid in t]
            evidence.append(
                f"unsafe blocks citing {cid}: " + (", ".join(citing) if citing else "none")
                + f"; unsafe blocks under src/ for this backend: {len(all_sites)} sites "
                f"({len(shared)} shared, {len(own)} this backend's), {cited_any} cite a clause "
                "(check/proofs/unsafe-registry.sh)"
            )
            if miri:
                evidence.append("check/proofs/miri.sh: the crate's tests under Miri, Tree Borrows")
        status, notes = status_of(methods, cov)
        if cid in ("Q1", "Q2") and q:
            if any((p50, p99) != ("null", "null") for d in q["bounds"].values() for p50, p99 in d.values()):
                claim += "; a bound is set in q.nix: compliance is the latency analysis's verdict, not computed here"
                status = "checked"
                evidence.append(f"lab/latency/q.nix: {len(q['placements'])} placements x {len(q['sizes'])} sizes, bounds set")
            else:
                claim += f"; no bound set on deployment {q['reference']}: measured, not claimed"
                evidence.append(
                    f"lab/latency/q.nix: {len(q['placements'])} placements x {len(q['sizes'])} sizes, every bound null"
                )
            locked = [
                f"{k}: {t} runs, {ok} ok"
                for k, (t, ok) in sorted(runs.items())
                if k.startswith(f"latency-{backend}")
            ]
            evidence.append("lab.lock: " + ("; ".join(locked) if locked else "no locked latency runs"))

        bound = [r["not_checked"]]
        bound.extend(notes)
        if cov:
            bound.append("a model is not the code, checked at its bounds only")
        if cid in MEMORY:
            bound.append(f"{uncited} of the backend's {len(all_sites)} unsafe blocks cite no clause")
            bound.append(
                "Miri runs the crate's tests only, loops shrunk under cfg!(miri)"
                if miri
                else "Miri does not run this backend: its tests need a launcher"
            )
        if cid in ("Q1", "Q2"):
            bound.append("a measurement is per deployment")
        rows_.append(
            {
                "backend": backend,
                "clause": cid,
                "claim": claim,
                "method": ", ".join(methods),
                "evidence": " ".join(evidence),
                "bound": "; ".join(b.strip(" ;") for b in bound if b.strip(" ;")),
                "status": status,
                "source": "generated",
            }
        )
    return rows_


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".", help="the tree the rows cite paths in")
    ap.add_argument("--out", required=True, help="where certificate.tsv and certificate.md are written")
    args = ap.parse_args()
    errors = []
    vocab = guarantees.vocabulary(args.root)
    clause_list = guarantees.clauses(args.root)
    clause_ids = [c for c, _ in clause_list]
    names, missing = guarantees.backends(args.root)
    for n in missing:
        errors.append(f"{n}: src/backends/{n}/guarantees.tsv is missing")

    out_rows = []
    for backend in names:
        gen = generate(args.root, backend, vocab, clause_list, errors)
        gen_methods = {r["clause"]: [t.strip() for t in r["method"].split(",")] for r in gen}
        for a in authored_rows(args.root, backend, vocab, clause_ids, errors):
            m = STATUS_METHOD.get(a["status"], "")
            if a["clause"] in gen_methods and m in gen_methods[a["clause"]]:
                errors.append(
                    f"{backend} {a['clause']}: an authored row of status {a['status']} (method {m}) "
                    f"duplicates the generated row's method"
                )
            out_rows.append(
                {
                    "backend": backend,
                    "clause": a["clause"],
                    "claim": a["claim"],
                    "method": m,
                    "evidence": a["evidence"],
                    "bound": a["bound"],
                    "status": a["status"],
                    "source": "authored",
                }
            )
        out_rows.extend(gen)

    order = {c: i for i, (c, _) in enumerate(clause_list)}
    # a row that failed validation sorts last and is never written: the errors above exit first
    out_rows.sort(key=lambda r: (names.index(r["backend"]), order.get(r["clause"], len(order)), r["source"]))
    for r in out_rows:
        if not r["bound"].strip():
            errors.append(f"{r['backend']} {r['clause']}: the bound is empty: a row without a limit is advertising")

    for e in errors:
        print(e)
    if errors:
        sys.exit(1)

    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "certificate.tsv"), "w") as f:
        f.write("\t".join(COLS) + "\n")
        for r in out_rows:
            f.write("\t".join(r[c] for c in COLS) + "\n")

    md = [
        "# The certificate",
        "",
        "One row per backend and clause of CONTRACT.md. `method` is how the row is checked; `status` is the",
        "strongest thing its methods establish; `bound` says what they do not reach, on every row. A `generated`",
        "row comes from the guarantee rows, the model inventory, the latency bounds, the lab lock, the unsafe",
        "registry and the counts; an `authored` row is read from the code and lives in",
        "src/backends/<backend>/certificates.tsv.",
        "",
    ]
    for backend in names:
        models = inventory(args.root, backend, errors)
        miri = miri_covers(args.root, backend)
        shared, own = unsafe_sites(args.root, backend)
        uncited = sum(1 for _, _, t in shared + own if not t)
        md += [f"## {backend}", ""]
        md.append("| " + " | ".join(COLS) + " |")
        md.append("|" + "---|" * len(COLS))
        for r in (r for r in out_rows if r["backend"] == backend):
            cells = [
                r["backend"],
                r["clause"],
                trim(r["claim"].split(";")[0].split(": ")[-1]),
                r["method"],
                trim(r["evidence"], 110),
                trim(r["bound"]),
                r["status"],
                r["source"],
            ]
            md.append("| " + " | ".join(c.replace("|", "\\|") for c in cells) + " |")
        md += ["", "**What this does not say**", ""]
        says = []
        if models:
            says.append(
                f"- A model is not the code: {len(models)} models of this backend's proofs are checked, at their "
                "bounds, of abstractions that follow the code by reading."
            )
        else:
            says.append("- No model of this backend is checked: nothing here is proven under a model.")
        if miri:
            says.append(
                "- Miri covers this backend's crate tests only: the launcher, the lab programs and the build are "
                "outside it, and its loops are shrunk under cfg!(miri)."
            )
        else:
            says.append("- Miri does not run this backend: its tests need a launcher.")
        says.append("- A measurement is per deployment: no latency number here holds of another machine or load.")
        says.append(
            f"- The unsafe registry is a reading, not a gate: {uncited} of this backend's {len(shared) + len(own)} "
            "unsafe blocks under src/ cite no clause, and citing one names an obligation, it does not discharge it."
        )
        says.append("- A finite trace shows that one run was allowed; no row shows that every run is.")
        md += says[:5] + [""]
    with open(os.path.join(args.out, "certificate.md"), "w") as f:
        f.write("\n".join(md))
    print(
        f"ok: {len(out_rows)} rows ({len(names)} backends x {len(clause_ids)} clauses plus "
        f"{sum(1 for r in out_rows if r['source'] == 'authored')} authored) into {args.out}"
    )


if __name__ == "__main__":
    main()
