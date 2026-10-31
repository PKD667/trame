#!/usr/bin/env bash
# The trame CLI against programs/*.expected: run each named program (default: three) on seq and on mpi with the
# rank count asked of the program, and compare stdout. Run in the MPI shell:
#   nix develop .#mpi --command cli/check.sh [program...]
set -euo pipefail
cd "$(dirname "$0")/.."
trame=cli/trame
progs=("$@"); [[ ${#progs[@]} -gt 0 ]] || progs=(ping_pong halo_rule90 fanin_fifo)
ex=(); for p in "${progs[@]}"; do ex+=(--example "$p"); done
$trame build "${ex[@]}" >/dev/null 2>&1
$trame build --mpi "${ex[@]}" >/dev/null 2>&1
for p in "${progs[@]}"; do
  diff -u "programs/$p.expected" <($trame run "$p") >/dev/null || { echo "FAIL seq $p"; exit 1; }
  diff -u "programs/$p.expected" <($trame run --mpi "$p" 2>/dev/null | uniq) >/dev/null || { echo "FAIL mpi $p"; exit 1; }
  echo "ok   $p"
done
