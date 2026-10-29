#!/usr/bin/env bash
# Runs every shared program on the seq and mpi backends and checks stdout against <name>.expected.
# Run from anywhere; the MPI half needs the Nix shell:
#   nix develop .#mpi --command programs/run.sh [seq|mpi|both]
# The argument selects the backend (default: both). Exits non-zero on the first failure, and prints
# "FAIL <backend> <program>" so conformance/mutate.sh can name the program that killed a mutant.
# The programs are the files programs/*.rs (seq) and the rows of backends/mpi/deploy/ranks.txt (mpi).
# Every program is built with --cfg trame_checked (concurrent! needs it). The MPI target directory is
# target/mpi, as in backends/mpi/tests/run.sh. Programs run as built binaries, so a timeout kills them.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
which_backend="${1:-both}"
case "$which_backend" in seq|mpi|both) ;; *) echo "usage: $0 [seq|mpi|both]" >&2; exit 2 ;; esac
export RUSTFLAGS='--cfg trame_checked'
# seq: every programs/*.rs. Each needs its [[example]] in Cargo.toml, checked here.
seq_programs=()
for f in "$root"/programs/*.rs; do
  p="$(basename "$f" .rs)"
  if ! awk -v n="$p" -v path="programs/$p.rs" '
      /^\[\[example\]\]/ { name = 0 }
      $0 == "name = \"" n "\"" { name = 1 }
      $0 == "path = \"" path "\"" && name { found = 1 }
      END { exit !found }' "$root/Cargo.toml"; then
    echo "FAIL missing: Cargo.toml has no [[example]] name = \"$p\" with path = \"programs/$p.rs\"" >&2
    exit 1
  fi
  seq_programs+=("$p")
done
# mpi: the rows of backends/mpi/deploy/ranks.txt, in its order: workers + 1 ranks per program. The launch
# refuses any other rank count (open.rs agree). A program with no row does not run on mpi.
# The table is shared with the nixsci deployment (default 3 ranks).
mpi_programs=()
declare -A RANKS=()
while read -r p r; do mpi_programs+=("$p"); RANKS[$p]=$r; done < <(grep -v '^#' "$root/backends/mpi/deploy/ranks.txt")
T=120
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; exit 1; }
# stderr of each run. A program with <name>.panics must end non-zero, with that line on its stderr (the
# panic of the leader, spec.md 5, Abort). Its stdout must equal <name>.expected when that file exists.
# Any other program exits 0 and its stdout must equal <name>.expected.
errf="$(mktemp)"
trap 'rm -f "$errf"' EXIT
verify() { # label program exit-status stdout
  local label=$1 p=$2 rc=$3 out=$4
  if [[ -f "programs/$p.panics" ]]; then
    if [[ "$rc" -eq 0 ]]; then echo "$out"; fail "$label: exit status 0, but a panic is expected"; fi
    if ! grep -F -x -q -- "$(cat "programs/$p.panics")" "$errf"; then cat "$errf" >&2; fail "$label: stderr lacks the panic line"; fi
    [[ -f "programs/$p.expected" ]] || { ok "$label"; return; }
  elif [[ "$rc" -ne 0 ]]; then
    fail "$label: exit status $rc"
  fi
  diff -u "programs/$p.expected" <(emit "$out") >/dev/null || { echo "$out"; fail "$label: stdout differs"; }
  ok "$label"
}
# The stdout of a run as its .expected file holds it: nothing when empty, else the lines.
emit() { if [[ -n "$1" ]]; then echo "$1"; fi; }

if [[ "$which_backend" != mpi ]]; then
  # seq: the default build, one process per run.
  cargo build -q --examples 2>&1 | grep -v 'Git tree' || true
  B="${CARGO_TARGET_DIR:-$root/target}/debug/examples"
  for p in "${seq_programs[@]}"; do
    rc=0
    out="$(timeout -k 5 "$T" "$B/$p" 2>"$errf")" || rc=$?
    verify "seq $p" "$p" "$rc" "$out"
  done
fi

if [[ "$which_backend" != seq ]]; then
  # mpi: W + 1 ranks, the leader last. Every rank runs the program's host code, so refuse prints once per
  # rank: the lines are collapsed to one. The exit status of mpirun is the program's, and must be 0.
  export CARGO_TARGET_DIR="$root/target/mpi"
  cargo build -q --no-default-features --features mpi --examples 2>&1 | grep -v 'Git tree' || true
  B="$CARGO_TARGET_DIR/debug/examples"
  # Every program also runs with TRAME_MPI_NODE_RANKS = 1 and 2: the node split of NODES.md, on one
  # machine. The output must not change with the split.
  for k in "" 1 2; do
    for p in "${mpi_programs[@]}"; do
      # `env -u` leaves the variable unset for the first pass: an empty value is refused (nodes.rs).
      if [[ -n "$k" ]]; then set_k=(env "TRAME_MPI_NODE_RANKS=$k"); else set_k=(env -u TRAME_MPI_NODE_RANKS); fi
      rc=0
      out="$("${set_k[@]}" timeout -k 5 "$T" mpirun --oversubscribe -n "${RANKS[$p]:-3}" "$B/$p" 2>"$errf")" || rc=$?
      out="$(echo "$out" | uniq)"
      verify "mpi ${k:+k=$k }$p" "$p" "$rc" "$out"
    done
  done
fi
echo "all programs passed on $which_backend"
