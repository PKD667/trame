#!/usr/bin/env bash
# P1 compares the exported item inventory, then checks the documented Rust contract as a client.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
here=$root/trame/scripts/surface
out=${1:?usage: surface.sh OUT}
mkdir -p "$out"
cd "$root"

features=(none nv mpi rma rma-lossy)
failures=0
compiler=$(command -v rustc)
rustdoc=$(command -v rustdoc)
cargo=$(command -v cargo)
rustc -Vv >"$out/rustc.version"
rustdoc --version >"$out/rustdoc.version"
sha256sum Cargo.lock trame/Cargo.toml >"$out/manifest.sha256"

# Every selection, including MPI, must use these exact compiler executables. Nix supplies MPI
# headers and libraries only; its compiler is never trusted by PATH or by assumption.
mpi_env() {
	nix develop -c env "RUSTC=$compiler" "RUSTDOC=$rustdoc" "$@"
}
run_cargo() {
	local feature=$1; shift
	local target=$1; shift
	if [[ $feature == mpi || $feature == rma || $feature == rma-lossy ]]; then
		mpi_env "CARGO_TARGET_DIR=$target" "$@"
	else
		CARGO_TARGET_DIR="$target" "$@"
	fi
}

# Check the selected executables in the MPI dependency shell before any build/comparison.
if command -v nix >/dev/null 2>&1; then
	if ! mpi_env sh -c '"$RUSTC" -Vv && "$RUSTDOC" --version && command -v mpicc && mpicc --showme:link' >"$out/mpi-environment.log" 2>&1; then
		echo "P1 setup failure: MPI shell does not expose the exact selected Rust tools and MPI" >&2
		cat "$out/mpi-environment.log" >&2
		exit 1
	fi
else
	echo "P1 setup failure: nix is required for MPI selections" >&2
	exit 1
fi

for feature in "${features[@]}"; do
	if [[ $feature == none ]]; then args=(-p trame --no-default-features); else args=(-p trame --no-default-features --features "$feature"); fi
	target=$out/target-$feature
	mkdir -p "$target"
	# Cargo's JSON compiler-artifact records are retained and select this build's rlib exactly.
	if ! run_cargo "$feature" "$target" "$cargo" build "${args[@]}" --locked --message-format=json-render-diagnostics >"$out/build-$feature.jsonl" 2>"$out/build-$feature.log"; then
		echo "P1 build failure: $feature" >&2; cat "$out/build-$feature.log" >&2; exit 1
	fi
	python3 - "$out/build-$feature.jsonl" "$out/artifact-$feature.txt" <<'PY'
import json, sys
records=[]
for line in open(sys.argv[1]):
    try: item=json.loads(line)
    except json.JSONDecodeError: continue
    if item.get("reason")=="compiler-artifact" and item.get("target",{}).get("name")=="trame":
        records.extend(path for path in item.get("filenames",[]) if path.endswith(".rlib"))
if len(records)!=1: raise SystemExit(f"expected exactly one trame rlib artifact, found {len(records)}")
open(sys.argv[2],"w").write(records[0]+"\n")
PY
	rl=$(<"$out/artifact-$feature.txt")
	if ! run_cargo "$feature" "$target" "$cargo" doc "${args[@]}" --no-deps >"$out/doc-$feature.stdout" 2>"$out/doc-$feature.log"; then
		echo "P1 rustdoc failure: $feature" >&2; cat "$out/doc-$feature.log" >&2; exit 1
	fi
	doc=$target/doc/trame/all.html
	if [[ ! -s $doc ]]; then echo "P1 inventory failure: missing/empty $doc" >&2; exit 1; fi
	python3 - "$doc" "$out/items-$feature.txt" <<'PY'
from html.parser import HTMLParser
from pathlib import PurePosixPath
import sys
class Links(HTMLParser):
    def __init__(self): super().__init__(); self.in_item=False; self.href=None; self.text=[]; self.items=[]
    def handle_starttag(self, tag, attrs):
        a=dict(attrs)
        if tag=="li": self.in_item=True
        elif tag=="a" and self.in_item and "href" in a: self.href=a["href"]; self.text=[]
    def handle_data(self, data):
        if self.href is not None: self.text.append(data)
    def handle_endtag(self, tag):
        if tag=="a" and self.href is not None:
            href=self.href.split("#",1)[0]
            filename=PurePosixPath(href).name
            stem=filename.removesuffix(".html")
            kind,name=stem.split(".",1) if "." in stem else ("item",stem)
            label=" ".join("".join(self.text).split())
            self.items.append((href,kind,name,label))
            self.href=None
        elif tag=="li": self.in_item=False
p=Links(); p.feed(open(sys.argv[1],encoding="utf8").read())
items=sorted(set(p.items))
if not items: raise SystemExit("empty rustdoc exported-item inventory")
with open(sys.argv[2],"w") as f:
    for href,kind,name,label in items: f.write(f"{href}\t{kind}\t{name}\t{label}\n")
PY
	printf '%s\n' "$compiler" >"$out/rustc-path-$feature.txt"
	printf '%s\n' "$rustdoc" >"$out/rustdoc-path-$feature.txt"
	done

for feature in nv mpi rma rma-lossy; do
	if ! diff -u "$out/items-none.txt" "$out/items-$feature.txt" >"$out/items-$feature.diff"; then
		echo "P1 exported-item mismatch: none vs $feature" >&2; cat "$out/items-$feature.diff" >&2; failures=$((failures+1))
	else : >"$out/items-$feature.diff"; fi
done

compile_fixture() {
	local feature=$1 fixture=$2 rlib deps status rl
	rl=$(<"$out/artifact-$feature.txt"); rlib=$rl; deps=$(dirname "$rlib")/deps
	local -a rustc_args=(--edition 2024 --crate-type lib --emit=metadata --error-format=json -L "dependency=$deps" --extern "trame=$rlib" -o "$out/$(basename "$fixture" .rs)-$feature.rmeta" "$fixture")
	if [[ $feature == mpi || $feature == rma || $feature == rma-lossy ]]; then
		printf 'nix develop -c env RUSTC=%q RUSTDOC=%q %q' "$compiler" "$rustdoc" "$compiler" >"$out/$(basename "$fixture" .rs)-$feature.command"
	else
		printf '%q' "$compiler" >"$out/$(basename "$fixture" .rs)-$feature.command"
	fi
	printf ' %q' "${rustc_args[@]}" >>"$out/$(basename "$fixture" .rs)-$feature.command"
	printf '\n' >>"$out/$(basename "$fixture" .rs)-$feature.command"
	if [[ $feature == mpi || $feature == rma || $feature == rma-lossy ]]; then
		if mpi_env "$compiler" "${rustc_args[@]}" >"$out/$(basename "$fixture" .rs)-$feature.json" 2>&1; then return 0; else return $?; fi
	else
		if "$compiler" "${rustc_args[@]}" >"$out/$(basename "$fixture" .rs)-$feature.json" 2>&1; then return 0; else return $?; fi
	fi
}

passes=0
negatives=0
: >"$out/witness-status.txt"
for feature in "${features[@]}"; do
	count=0
	for fixture in "$here"/pass/*.rs; do
		[[ -f $fixture ]] || { echo "P1 failure: no positive fixtures" >&2; exit 1; }
		count=$((count+1)); passes=$((passes+1))
		if compile_fixture "$feature" "$fixture"; then
			printf 'PASS %s %s\n' "$feature" "$(basename "$fixture")" >>"$out/witness-status.txt"
		else
			status=$?; printf 'FAIL %s %s EXIT %s\n' "$feature" "$(basename "$fixture")" "$status" >>"$out/witness-status.txt"
			failures=$((failures+1))
		fi
	done
	[[ $count -gt 0 ]] || { echo "P1 failure: zero positive fixtures" >&2; exit 1; }
	count=0
	for fixture in "$here"/fail/*.rs; do
		[[ -f $fixture ]] || { echo "P1 failure: no negative fixtures" >&2; exit 1; }
		count=$((count+1)); negatives=$((negatives+1))
		name=$(basename "$fixture" .rs)
		if compile_fixture "$feature" "$fixture"; then
			printf 'FAIL %s %s unexpectedly accepted\n' "$feature" "$name" >>"$out/witness-status.txt"; failures=$((failures+1)); continue
		fi
		json="$out/$name-$feature.json"; source_line=$(grep -n 'P1-OBLIGATION:' "$fixture" | cut -d: -f1); expectation=$(sed -n 's/.*P1-OBLIGATION: *\([^ ]*\) *\([^ ]*\).*/\1 \2/p' "$fixture")
		if [[ -z $source_line || -z $expectation ]] || ! python3 - "$json" "$source_line" $expectation <<'PY'
import json,sys
path,line,code,claim=sys.argv[1],int(sys.argv[2]),sys.argv[3],sys.argv[4]
errors=[]
for raw in open(path):
    try: m=json.loads(raw)
    except json.JSONDecodeError: continue
    if m.get("level")=="warning": raise SystemExit("unexpected compiler warning")
    if m.get("level")=="error" and (m.get("code") or {}).get("code"): errors.append(m)
if not errors: raise SystemExit("no structured coded rustc error")
for e in errors:
    spans=[s for s in e.get("spans",[]) if s.get("is_primary")]
    if e.get("code",{}).get("code")!=code or not any(s.get("line_start")==line for s in spans): raise SystemExit("wrong rustc error code or primary obligation span")
    text=e.get("message","")+" "+" ".join(s.get("label","") for s in spans)+" "+" ".join(c.get("message","") for c in e.get("children",[]))
    if claim not in text: raise SystemExit("diagnostic does not identify expected obligation: "+text)
PY
		then
			printf 'FAIL %s %s wrong diagnostic EXIT nonzero\n' "$feature" "$name" >>"$out/witness-status.txt"; failures=$((failures+1))
		else printf 'PASS %s %s expected %s refusal\n' "$feature" "$name" "$expectation" >>"$out/witness-status.txt"; fi
	done
	[[ $count -gt 0 ]] || { echo "P1 failure: zero negative fixtures" >&2; exit 1; }
done

printf 'P1 counts: %s positive compilation attempts, %s negative compilation attempts; failures=%s\n' "$passes" "$negatives" "$failures" | tee "$out/counts.txt"
if (( failures )); then exit 1; fi
exit 0
