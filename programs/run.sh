#!/usr/bin/env bash
# Runs every shared program on the seq and mpi backends and checks stdout against <name>.expected.
# Run from anywhere; the MPI half needs the Nix shell:
#   nix develop .#mpi --command programs/run.sh [seq|mpi|both]
# The argument selects the backend (default: both). Exits non-zero on the first failure, and prints
# "FAIL <backend> <program>" so conformance/mutate.sh can name the program that killed a mutant.
# Every program is built with --cfg trame_checked (concurrent! needs it). The MPI target directory is
# target/mpi, as in backends/mpi/tests/run.sh. Programs run as built binaries, so a timeout kills them.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
which_backend="${1:-both}"
case "$which_backend" in seq|mpi|both) ;; *) echo "usage: $0 [seq|mpi|both]" >&2; exit 2 ;; esac
export RUSTFLAGS='--cfg trame_checked'
programs=(ping_pong leader_io full lossy abort refuse barrier_publish barrier_deadlock cells wait_frame wait_room wait_until err join_deadline)
# MPI ranks per program: workers + 1. The launch refuses any other rank count (open.rs agree). Default 3.
# The table is backends/mpi/deploy/ranks.txt, shared with the nixsci deployment (default 3 ranks).
declare -A RANKS=()
while read -r p r; do RANKS[$p]=$r; done < <(grep -v '^#' "$root/backends/mpi/deploy/ranks.txt")
T=120
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; exit 1; }

if [[ "$which_backend" != mpi ]]; then
  # seq: the default build, one process per run.
  cargo build -q --examples 2>&1 | grep -v 'Git tree' || true
  B="$root/target/debug/examples"
  for p in "${programs[@]}"; do
    out="$(timeout -k 5 "$T" "$B/$p" 2>/dev/null)" || fail "seq $p: exit status"
    diff -u "programs/$p.expected" <(echo "$out") >/dev/null || { echo "$out"; fail "seq $p: stdout differs"; }
    ok "seq $p"
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
    for p in "${programs[@]}"; do
      # `env -u` leaves the variable unset for the first pass: an empty value is refused (nodes.rs).
      if [[ -n "$k" ]]; then set_k=(env "TRAME_MPI_NODE_RANKS=$k"); else set_k=(env -u TRAME_MPI_NODE_RANKS); fi
      out="$("${set_k[@]}" timeout -k 5 "$T" mpirun --oversubscribe -n "${RANKS[$p]:-3}" "$B/$p" 2>/dev/null)" || fail "mpi ${k:+k=$k }$p: mpirun exit status"
      out="$(echo "$out" | uniq)"
      diff -u "programs/$p.expected" <(echo "$out") >/dev/null || { echo "$out"; fail "mpi ${k:+k=$k }$p: stdout differs"; }
      ok "mpi ${k:+k=$k }$p"
    done
  done
fi
echo "all programs passed on $which_backend"
