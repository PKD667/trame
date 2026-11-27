#!/usr/bin/env python3
"""Measure the module structure of this crate: modules and depths, the `crate::` import
graph, the pub / pub(crate) surface, per-backend import direction, and the names the
macros crate emits into the crate root.
Prints one JSON document (or short text); edits nothing. Backend names are discovered
from `backend.nix` files, never written here (the rule of nix/names.nix).

Usage: python3 check/modules.py [--root REPO] [--json]
"""

from __future__ import annotations
import argparse
import json
import re
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

# --------------------------------------------------------------------- comments / string bodies


def strip_comments(text: str) -> str:
    """Blank out comments and string bodies, keeping each character's position."""
    out = list(text)
    n = len(text)
    i = 0
    while i < n:
        if text.startswith("//", i):
            j = i
            while j < n and text[j] != "\n":
                out[j] = " "
                j += 1
            i = j
        elif text.startswith("/*", i):
            j = i
            while j < n and not text.startswith("*/", j):
                if text[j] != "\n":
                    out[j] = " "
                j += 1
            for k in range(i, min(j + 2, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j + 2
        elif text[i] == '"':
            m = re.compile(r"r(#*)\Z").search(text[:i])
            if m and text[:i].endswith("r" + "#" * len(m.group(1))):
                closer = '"' + "#" * len(m.group(1))
                j = text.find(closer, i + 1)
                j = n if j < 0 else j + len(closer) - 1
            else:  # plain string; honour escapes
                j = i + 1
                while j < n and text[j] != '"':
                    j += 2 if text[j] == "\\" else 1
            for k in range(i, min(j + 1, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j + 1
        else:
            i += 1
    return "".join(out)


# ------------------------------------------------------------------------------- module resolver

MOD_DECL = re.compile(r"(?m)^\s*(?:pub(?:\(\s*[a-z/:]+\s*\))?\s+)?(?:unsafe\s+)?mod\s+(\w+)\s*;")
PATH_ATTR = re.compile(r'#\[\s*path\s*=\s*"([^"]+)"\]')
CRATE_PATH = re.compile(r"(?<![:A-Za-z0-9_])crate::([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)")
KINDS = ("lib", "bin", "example")


@dataclass
class Mod:
    path: tuple[str, ...]  # from the crate root; () is the root itself
    crate: str  # the crate-root label this module compiles into
    files: list[Path] = field(default_factory=list)

    @property
    def name(self) -> str:
        return "::".join(self.path) if self.path else "(root)"

    def depth(self) -> int:
        return len(self.path)


def mod_decls(code: str, raw: str):
    """(name, #[path] value or "", offset) for each `mod name;` in one file. Offsets come
    from the comment-stripped code; the `#[path]` value is read from the raw text, because
    stripping blanks string bodies."""
    out = []
    raw_lines = raw.splitlines()
    for m in MOD_DECL.finditer(code):
        attr = ""
        at = raw_lines[: code.count("\n", 0, m.start())]
        for line in reversed(at[-3:]):
            if line.strip().startswith("#["):
                am = PATH_ATTR.search(line)
                if am:
                    attr = am.group(1)
            elif line.strip():
                break
        out.append((m.group(1), attr, m.start()))
    return out


def resolve(crate: str, root_file: Path, prefix: tuple[str, ...] = ()):
    """Module tree of one crate root. `prefix` mounts the root file at a deeper path: the
    `mod` lines that name the backend modules are generated into $OUT_DIR, so each such
    directory is resolved from its own mod.rs at the place build.rs declares it."""
    mods: dict[tuple[str, ...], Mod] = {}
    missing: list[str] = []
    todo = [(prefix, root_file)]
    while todo:
        mpath, file = todo.pop()
        file = file.resolve()
        mod = mods.setdefault(mpath, Mod(mpath, crate))
        if file in mod.files:
            continue
        mod.files.append(file)
        raw = file.read_text()
        code = strip_comments(raw)
        base = file.parent
        for name, attr, off in mod_decls(code, raw):
            stem = file.stem  # `foo.rs` holds its submodules in `foo/`, `mod.rs` beside itself
            sub = file.name == "mod.rs" and (base / name).is_dir()
            if attr:
                todo.append((mpath + (name,), base / attr))
            elif (base / f"{name}.rs").exists():
                todo.append((mpath + (name,), base / f"{name}.rs"))
            elif (base / name / "mod.rs").exists():
                todo.append((mpath + (name,), base / name / "mod.rs"))
            elif (base / stem / f"{name}.rs").exists():
                todo.append((mpath + (name,), base / stem / f"{name}.rs"))
            else:
                missing.append(f"{file}:{code.count(chr(10), 0, off) + 1}: mod {name}; has no file")
    return mods, missing


def package_roots(repo: Path) -> dict[str, list[Path]]:
    """Packages found by walking the tree for Cargo.toml (skipping target/); each maps to
    the .rs roots its manifest names: [lib].path, [[bin]].path, [[example]].path. Keyed by
    the manifest's directory (two packages here carry the same cargo name)."""
    roots: dict[str, list[Path]] = {}
    for toml in sorted(repo.rglob("Cargo.toml")):
        if "target" in toml.parts or ".git" in toml.parts:
            continue
        text = toml.read_text()
        name = re.search(r'(?m)^name\s*=\s*"([^"]+)"', text)
        if not name:
            continue
        found: list[Path] = []
        header, section = None, []

        def flush():
            if header in KINDS:
                for pm in re.finditer(r'(?m)^\s*path\s*=\s*"([^"]+)"', "\n".join(section)):
                    p = toml.parent / pm.group(1)
                    if p.suffix == ".rs" and p.exists() and p not in found:
                        found.append(p)

        for raw in text.splitlines():
            hm = re.match(r"^\[{1,2}([^\[\]]+)\]{1,2}\s*$", raw)
            if hm:
                flush()
                header = hm.group(1).split(".")[0].strip()
                section = []
            elif header is not None:
                section.append(raw)
        flush()
        # manifests that rely on cargo's defaults: src/lib.rs, src/main.rs, examples/<name>.rs
        for default in (toml.parent / "src" / "lib.rs", toml.parent / "src" / "main.rs"):
            if default.exists() and default not in found:
                found.append(default)
        if found:
            label = str(toml.parent.relative_to(repo))
            roots.setdefault(label, []).extend(p.resolve() for p in found)
    return roots


# ----------------------------------------------------------------------------------------- run

PUB_ITEM = re.compile(
    r"\b(pub(?:\(\s*crate\s*\))?|pub(?:\(\s*super\s*\))?)\s+(?:unsafe\s+)?(?:extern\s+)?"
    r"(const\s+fn|const|static|fn|struct|enum|union|trait|type|use|macro)\s+([A-Za-z_][A-Za-z0-9_]*)"
)
PUB_MOD = re.compile(r"\b(pub(?:\(\s*crate\s*\))?|pub(?:\(\s*super\s*\)))\s+(?:unsafe\s+)?mod\s+(\w+)\s*;")


def items_of(code: str):
    for m in PUB_ITEM.finditer(code):
        yield m.group(1), m.group(2), m.group(3), m.start()
    for m in PUB_MOD.finditer(code):
        yield m.group(1), "mod", m.group(2), m.start()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()
    repo = Path(args.root).resolve()
    root_name = re.search(r'(?m)^name\s*=\s*"([^"]+)"', (repo / "Cargo.toml").read_text()).group(1)

    packages = package_roots(repo)
    backends_dir = repo / "src" / "backends"
    backend_names = (
        sorted(d.name for d in backends_dir.iterdir() if d.is_dir() and (d / "backend.nix").exists())
        if backends_dir.is_dir()
        else []
    )

    # module trees: the lib of every package, plus every bin and example root
    trees: dict[str, dict[tuple[str, ...], Mod]] = {}
    trees_missing: list[str] = []
    for pkgdir, roots in sorted(packages.items()):
        for root_file in roots:
            label = (root_name if pkgdir == "." else pkgdir) + "::" + root_file.stem
            tree, missing = resolve(label, root_file)
            trees[label] = tree
            trees_missing.extend(missing)
    lib_label = f"{root_name}::lib"

    # backend directories whose `mod` line is generated: resolve from their mod.rs, mounted
    # where build.rs declares them. No explicit name of a backend appears here for that.
    for b in backend_names:
        modrs = (backends_dir / b / "mod.rs").resolve()
        if not modrs.exists() or any(
            modrs in m.files for tree in trees.values() for m in tree.values()
        ):
            continue
        tree, missing = resolve(lib_label, modrs, prefix=("backends", b))
        trees[lib_label].update(tree)
        trees_missing.extend(missing)

    # every resolved file -> its crate root and module path
    where: dict[Path, tuple[str, tuple[str, ...]]] = {}
    for label, tree in trees.items():
        for m in tree.values():
            for f in m.files:
                where[f] = (label, m.path)

    stripped: dict[Path, str] = {}

    def code_of(f: Path) -> str:
        if f not in stripped:
            stripped[f] = strip_comments(f.read_text())
        return stripped[f]

    # roots on the tree no one reaches: standalone artifacts (labs, drivers), reported
    orphans = [
        str(f.relative_to(repo))
        for f in sorted((repo / "src").rglob("*.rs"))
        if "target" not in f.parts
        and f not in where
        and re.search(r"\bfn\s+main\s*\(", code_of(f))
    ]

    # ------------------------------------------------------------------ import edges
    macro_root_names = set(
        re.findall(r"pub use trame_macros::(?:\w+|\{[^}]*\})", code_of(repo / "src" / "lib.rs"))
        and [n for group in re.findall(r"pub use trame_macros::(?:\w+|\{([^}]*)\})", code_of(repo / "src" / "lib.rs")) for n in group.split(",")]
        + re.findall(r"pub use trame_macros::(\w+)", code_of(repo / "src" / "lib.rs"))
    )
    # names the src root re-exports from the macros crate: entry points of `#[trame::...]`
    # and derive paths (`#[derive(crate::Object)]`), not module imports
    macro_root_names = {n.strip() for n in re.findall(
        r"pub use trame_macros::(?:[\w:]+|\{([^}]*)\})", code_of(repo / "src" / "lib.rs"))
        if False}
    for _found in [re.finditer(
        r"pub use trame_macros::(?:(\w+)|\{([^}]*)\})",
        re.sub(r"pub use trame_macros::\w+;", "", ""),
    )]:
        pass
    def _macro_names(root_code):
        names = set()
        for m in re.finditer(r"pub use trame_macros::(?:(\w+)|\{([^}]*)\})", root_code):
            names.add(m.group(1)) if m.group(1) else names.update(
                n.strip() for n in m.group(2).split(",")
            )
        return names
    macro_root_names = _macro_names(code_of(repo / "src" / "lib.rs"))
    quant = []
    edges = []
    for file in sorted(where):
        label, mpath = where[file]
        tree = trees[label]
        code = code_of(file)
        for m in CRATE_PATH.finditer(code):
            head = code[max(0, m.start() - 4) : m.start()]
            if "#[" in head.replace(" ", "").replace("\n", ""):
                continue  # `#[crate::step]`: a macro entry point, not an import
            segs = tuple(m.group(1).split("::"))
            target, tlen = (), 0
            for k in range(len(segs), 0, -1):
                if segs[:k] in tree:
                    target, tlen = segs[:k], k
                    break
            kind = "import"
            if target == () and m.group(1).split("::")[-1] in macro_root_names:
                kind = "macro"  # `#[derive(crate::Object)]`, `crate::step`: a macro entry point
            edges.append(
                {
                    "file": str(file.relative_to(repo)),
                    "crate": label,
                    "kind": kind,
                    "from": "::".join(mpath) if mpath else "(root)",
                    "to": "::".join(target) if target else "(root)",
                    "to_seg": target,
                    "levels": tlen,
                    "path": "crate::" + m.group(1),
                    "line": code.count("\n", 0, m.start()) + 1,
                }
            )
    lib_all = [e for e in edges if e["crate"] == lib_label]
    lib_edges = [e for e in lib_all if e["kind"] == "import"]
    lib_macro_refs = [e for e in lib_all if e["kind"] == "macro"]
    uniq = len({(e["file"], e["to"]) for e in lib_edges})
    deep = sorted(
        (e for e in lib_edges if e["levels"] >= 3),
        key=lambda e: (-e["levels"], e["path"], e["file"]),
    )

    # hubs: distinct source files per target module, then raw references
    hub_files = defaultdict(set)
    hub_refs = defaultdict(int)
    for e in lib_edges:
        hub_files[e["to"]].add(e["file"])
        hub_refs[e["to"]] += 1
    top_hubs = sorted(hub_files.items(), key=lambda kv: (-len(kv[1]), kv[0]))

    # cycles of length <= 4 in the module graph (an edge import between modules)
    adj: dict[str, set[str]] = defaultdict(set)
    for e in lib_edges:
        if e["from"] != e["to"]:
            adj[e["from"]].add(e["to"])
    cycles: list[list[str]] = []
    seen_cycles = set()

    def walk(start: str, node: str, path: list[str]):
        for nxt in sorted(adj.get(node, ())):
            if nxt == start and 2 <= len(path) <= 4:
                key = tuple(sorted(path))
                if key not in seen_cycles:
                    seen_cycles.add(key)
                    cycles.append(path + [start])
            elif nxt not in path and len(path) < 4:
                walk(start, nxt, path + [nxt])

    for m in sorted(adj):
        walk(m, m, [m])

    per_file = defaultdict(set)
    per_file_refs = defaultdict(int)
    for e in lib_edges:
        per_file[e["file"]].add(e["to"])
        per_file_refs[e["file"]] += 1
    longest = sorted(per_file.items(), key=lambda kv: (-len(kv[1]), kv[0]))

    # ------------------------------------------------------------------ surface
    lib_tree = trees[lib_label]
    surface = []
    named_pubcrate: dict[str, tuple[Path, str, int]] = {}
    for m in sorted(lib_tree.values(), key=lambda m: m.name):
        npub = npubcrate = npubsuper = 0
        for f in m.files:
            code = code_of(f)
            for viz, kind, name, off in items_of(code):
                if "(crate" in viz:
                    npubcrate += 1
                    named_pubcrate.setdefault(name, (f, kind, code.count("\n", 0, off) + 1))
                elif "(super" in viz:
                    npubsuper += 1
                else:
                    npub += 1
        surface.append(
            {
                "module": m.name,
                "depth": m.depth(),
                "files": len(m.files),
                "lines": sum(len(code_of(f).splitlines()) for f in m.files),
                "pub": npub,
                "pub(crate)": npubcrate,
                "pub(super)": npubsuper,
            }
        )
    # pub(crate) items whose name a file in another directory reads (word-level: an upper
    # bound, an ordinary word reads as a reference); and the same count through a
    # qualified `crate::<module>::...` path (a lower bound)
    cross_reads = []
    for name, (file, kind, line) in sorted(named_pubcrate.items()):
        readers = []
        same_dir_readers = 0
        qualified = 0
        defining = where[file][1]
        for rf in sorted(where):
            if where[rf][0] != lib_label or rf == file:
                continue
            if not re.search(r"(?<![A-Za-z0-9_])" + re.escape(name) + r"(?![A-Za-z0-9_])", code_of(rf)):
                continue
            if rf.parent == file.parent:
                same_dir_readers += 1
            else:
                readers.append(str(rf.relative_to(repo)))
        qual = []
        for e in lib_edges:
            if e["to_seg"] == defining and e["path"].rsplit("::", 1)[-1] == name:
                if (repo / e["file"]).parent != file.parent:
                    qual.append(e["file"] + ":" + str(e["line"]))
        qualified = len(set(qual))
        if True:
            cross_reads.append(
                {
                    "item": name,
                    "kind": kind,
                    "defined": str(file.relative_to(repo)),
                    "at": line,
                    "different_dir_readers": len(readers),
                    "same_dir_readers": same_dir_readers,
                    "qualified_readers": qualified,
                    "examples": readers[:2],
                }
            )

    # ------------------------------------------------------------------ the seam and the backends
    seam_file = repo / "src" / "backend.rs"
    seam_items = []
    seam_trait = {}
    if seam_file.exists():
        code = code_of(seam_file)
        seam_items = [
            [kind, name, code.count("\n", 0, off) + 1]
            for viz, kind, name, off in items_of(code)
            if "(crate" in viz
        ]
        tm = re.search(r"\btrait\s+Backend\s*(?:\{|\bwhere|\s*\{)", code)
        if tm:
            close = re.search(r"(?m)^\}", code[tm.end() :])
            body = code[tm.end() : tm.end() + close.start()] if close else code[tm.end() :]
            seam_trait = {
                "methods": len(re.findall(r"(?m)^\s*(?:unsafe\s+)?fn\s+\w+", body)),
                "consts": len(re.findall(r"(?m)^\s*const\s+\w+", body)),
                "defaults": len(re.findall(r"(?m)^\s*(?:unsafe\s+)?fn\s+\w+[^;]*\{", body)),
            }

    def mod_dir(path_seg) -> Path | None:
        m = lib_tree.get(path_seg)
        return Path(m.files[0]).parent if m and m.files else None

    directions = {}
    for b in backend_names:
        bdir = (backends_dir / b).resolve()
        inside = to_root = outside = relative = 0
        for e in lib_edges:
            f = (repo / e["file"]).resolve()
            if bdir not in f.parents:
                continue
            if e["to"] == "(root)":
                to_root += 1
            elif mod_dir(e["to_seg"]) and mod_dir(e["to_seg"]).is_relative_to(bdir):
                inside += 1
            else:
                outside += 1
        for rf in sorted(where):
            if where[rf][0] != lib_label or bdir not in rf.parents:
                continue
            relative += len(re.findall(r"(?<![:A-Za-z0-9_])(?:super|self)::", code_of(rf)))
        directions[b] = {
            "absolute_to_own_tree": inside,
            "absolute_to_root": to_root,
            "absolute_to_core": outside,
            "super_self": relative,
        }

    core_reads_backend = [
        [e["file"], e["line"], e["path"]]
        for e in lib_edges
        if e["to_seg"] and e["to_seg"][0] == "backends"
        and backends_dir not in (repo / e["file"]).resolve().parents
    ]

    # ------------------------------------------------------------------ macros
    macros_sites: dict[str, list[str]] = {}
    for f in sorted((repo / "crates" / "macros").glob("*.rs")):
        code = code_of(f)
        for m in re.finditer(r"::trame::([A-Za-z_][A-Za-z0-9_]*)", code):
            macros_sites.setdefault(m.group(1), []).append(
                f"crates/macros/{f.name}:{code.count(chr(10), 0, m.start()) + 1}"
            )
    facade = re.findall(r"pub use trame_macros::(\w+)", code_of(repo / "src" / "lib.rs"))

    # ------------------------------------------------------------------ line baseline
    line_counts = sorted(
        (
            {
                "file": str(f.relative_to(repo)),
                "module": "::".join(p) if p else "(root)",
                "lines": len(f.read_text().splitlines()),
            }
            for f, (label, p) in sorted(where.items())
            if label == lib_label
        ),
        key=lambda r: -r["lines"],
    )

    out = {
        "command": "python3 check/modules.py",
        "python": sys.version.split()[0],
        "packages": {n: [str(r.relative_to(repo)) for r in rs] for n, rs in packages.items()},
        "backend_dirs": sorted(backend_names),
        "crate_roots": sorted(trees),
        "modules": [
            {
                "crate": label,
                "module": m.name,
                "depth": m.depth(),
                "files": [str(x.relative_to(repo)) for x in m.files],
            }
            for label, tree in sorted(trees.items())
            for m in sorted(tree.values(), key=lambda m: m.name)
        ],
        "unresolved_mod_decls": trees_missing,
        "orphan_mains": orphans,
        "edges": {
            "lib": {
                "distinct_file_to_module": uniq,
                "references": len(lib_all),
                "import_references": len(lib_edges),
                "macro_path_references": len(lib_macro_refs),
                "macro_path_names": sorted({e["path"].rsplit("::", 1)[-1] for e in lib_macro_refs}),
            },
            "all_crates": {
                "distinct": len({(e["file"], e["to"]) for e in edges}),
                "references": len(edges),
            },
            "depth_3plus": deep,
            "hubs": [
                {"module": m, "source_files": len(s), "references": hub_refs[m]} for m, s in top_hubs
            ],
            "cycles_length_lte_4": cycles,
            "longest_import_lists": [
                {"file": f, "distinct_modules": len(t), "references": per_file_refs[f]}
                for f, t in longest
            ],
        },
        "surface": surface,
        "pubcrate_items_named": len(named_pubcrate),
        "cross_read": sorted(cross_reads, key=lambda r: (-r["different_dir_readers"], r["item"])),
        "seam_pubcrate_items": seam_items,
        "seam_trait_backend": seam_trait,
        "backend_directions": directions,
        "core_reads_backend": core_reads_backend,
        "macros_emitted": {
            k: {"count": len(v), "sites": v[:6]} for k, v in sorted(macros_sites.items(), key=lambda kv: -len(kv[1]))
        },
        "macros_root_reexports": facade,
        "line_counts": line_counts,
    }

    if args.json:
        print(json.dumps(out, indent=1))
        return 0
    print(f"packages: {len(packages)}  crate roots: {len(trees)}  backends: {len(backend_names)}")
    print(f"modules of the lib: {len(trees[lib_label])}  orphan main() files: {orphans}")
    print(f"lib edges: {uniq} distinct / {len(lib_edges)} refs; depth>=3: {len(deep)}")
    print(
        f"cycles (<=4): {len(cycles)}; pub(crate) names: {len(cross_reads)},"
        f" word-read outside own dir: {sum(1 for r in cross_reads if r['different_dir_readers'])},"
        f" qualified-path read: {sum(1 for r in cross_reads if r['qualified_readers'])},"
        f" read by no other file: {sum(1 for r in cross_reads if not (r['different_dir_readers'] or r['same_dir_readers'] or r['qualified_readers']))}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
