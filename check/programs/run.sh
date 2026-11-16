#!/usr/bin/env bash
# check/programs/run.sh BACKEND [HOSTS...]: build every program of check/programs/ for BACKEND and run it on each
# host count of HOSTS (default 1 2), as trame hosts of this machine. BACKEND is a directory of src/backends/ that
# has a `launch` (src/backends/README.md); its cargo feature has its name. Run in that backend's shell:
#   nix develop .#BACKEND --command check/programs/run.sh BACKEND
# Prints "ok <backend> <h>h <program>" per run and stops at the first "FAIL ...", which check/mutants/mutate.sh
# reads to name the program that killed a mutant. A host count the backend refuses (launch exits 2) is skipped.
#
# Judging, the same rule as lab/lib/judge.py. A program with <name>.panics must end non-zero with that line on the
# stderr of some host; any other must exit 0. On one host its stdout must equal <name>.expected. On H hosts the
# hosts' outputs in order must equal <name>.h<H>.expected when it exists; a program with an .h<K>.expected for
# another K only is skipped; any other program's every host must equal <name>.expected.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
b="${1:?usage: $0 BACKEND [HOSTS...]}"
shift
counts=("${@:-1 2}")
read -r -a counts <<<"${counts[*]}"
launch="$root/src/backends/$b/launch"
[[ -x $launch ]] || { echo "FAIL $b: no executable src/backends/$b/launch" >&2; exit 2; }
export RUSTFLAGS='--cfg trame_checked'
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/$b}"
programs=()
for f in "$root"/check/programs/*.rs; do
  p="$(basename "$f" .rs)"
  if ! awk -v n="$p" -v path="check/programs/$p.rs" '
      /^\[\[example\]\]/ { name = 0 }
      $0 == "name = \"" n "\"" { name = 1 }
      $0 == "path = \"" path "\"" && name { found = 1 }
      END { exit !found }' "$root/Cargo.toml"; then
    echo "FAIL missing: Cargo.toml has no [[example]] name = \"$p\" with path = \"check/programs/$p.rs\"" >&2
    exit 1
  fi
  # A backend may list programs it does not run, one per line, with the reason (src/backends/README.md).
  if [[ -f src/backends/$b/skip.txt ]] && grep -q "^$p\b" "src/backends/$b/skip.txt"; then continue; fi
  programs+=("$p")
done
# A build that fails stops the script: a stale binary would otherwise pass the checks.
if ! out="$(cargo build -q --no-default-features --features "$b" --examples 2>&1)"; then
  echo "$out" >&2; echo "FAIL build: $b" >&2; exit 1
fi
B="$CARGO_TARGET_DIR/debug/examples"
T=120
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; exit 1; }
emit() { if [[ -n "$1" ]]; then echo "$1"; fi; }
for h in "${counts[@]}"; do
  for p in "${programs[@]}"; do
    d="$(mktemp -d)"
    label="$b ${h}h $p"
    if [[ $h -gt 1 && -f check/programs/$p.h$h.expected ]]; then exp="check/programs/$p.h$h.expected"
    elif compgen -G "check/programs/$p.h*.expected" >/dev/null && { [[ $h -gt 1 ]] || [[ ! -f check/programs/$p.expected ]]; }; then
      rm -rf "$d"; continue
    else exp=""; fi
    rc=0
    "$launch" --split "$h" --timeout "$T" --out "$d" -- "$B/$p" >/dev/null 2>"$d/launch.err" || rc=$?
    if [[ $rc -eq 2 && ! -f $d/host0.out ]]; then echo "skip $label: the backend refuses $h hosts"; rm -rf "$d"; break; fi
    [[ $rc -ne 124 ]] || fail "$label: timeout after $T s"
    if [[ -f check/programs/$p.panics ]]; then
      [[ $rc -ne 0 ]] || fail "$label: exit status 0, but a panic is expected"
      grep -F -x -q -- "$(cat "check/programs/$p.panics")" "$d"/host*.err || { cat "$d"/host*.err >&2; fail "$label: no host's stderr has the panic line"; }
    elif [[ $rc -ne 0 ]]; then
      cat "$d"/host*.err "$d/launch.err" >&2; fail "$label: exit status $rc"
    fi
    if [[ -n $exp ]]; then
      diff -u "$exp" <(for ((x = 0; x < h; x++)); do cat "$d/host$x.out"; done) >&2 || fail "$label: stdout differs"
    elif [[ -f check/programs/$p.expected ]]; then
      for ((x = 0; x < h; x++)); do
        diff -u "check/programs/$p.expected" <(emit "$(cat "$d/host$x.out")") >&2 || fail "$label: host $x stdout differs"
      done
    fi
    ok "$label"
    rm -rf "$d"
  done
done
echo "all programs passed on $b"
