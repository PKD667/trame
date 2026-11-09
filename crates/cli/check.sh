#!/usr/bin/env bash
# The trame CLI against check/programs/*.expected: build the named programs (default: three) for each backend and run
# them through its launch, comparing stdout. Backends: -b NAME (repeatable), or every backend of src/backends/.
#   nix develop .#<backend> --command crates/cli/check.sh [-b NAME]... [program...]
set -euo pipefail
cd "$(dirname "$0")/../.."
trame=crates/cli/trame
backends=(); progs=()
while [[ $# -gt 0 ]]; do
  case $1 in -b) backends+=("$2"); shift 2 ;; *) progs+=("$1"); shift ;; esac
done
[[ ${#backends[@]} -gt 0 ]] || mapfile -t backends < <($trame backends)
[[ ${#progs[@]} -gt 0 ]] || progs=(ping_pong halo_rule90 fanin_fifo)
ex=(); for p in "${progs[@]}"; do ex+=(--example "$p"); done
for b in "${backends[@]}"; do
  $trame build --backend "$b" "${ex[@]}" >/dev/null 2>&1 || { echo "FAIL $b build"; exit 1; }
  for p in "${progs[@]}"; do
    # A launch that collapses the ranks' duplicate lines (a refusal prints once per rank) does not alter a program's output.
    diff -u "check/programs/$p.expected" <($trame run --backend "$b" "$p" 2>/dev/null) >/dev/null || { echo "FAIL $b $p"; exit 1; }
    echo "ok   $b $p"
  done
done
