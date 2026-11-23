#!/usr/bin/env bash
# check/workers.sh BACKEND [PROGRAM]: hosts of different sizes (INTERFACE.md 6). Runs check/programs/workers through the
# backend's launch with --workers 3,1,2 on three hosts of this machine, 1,4 on two, and TRAME_WORKERS=2,5 on two,
# and checks that the program agrees with itself (its output) and that each host got ranks = its workers + 1
# (cpu.tsv). Run in the backend's shell, after check/programs/run.sh built the examples:
#   nix develop .#mpi --command check/workers.sh mpi
# PROGRAM is workers (the default) or agree (the leaders agree on a plan before the launch, check/programs/agree.rs).
# A backend that runs one host refuses these shapes (launch exits 2): the script prints "skip" and exits 0.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
b="${1:?usage: $0 BACKEND [PROGRAM]}"
program="${2:-workers}"
# The most workers a host declares: workers caps them, agree declares what the plan says.
cap=6; [[ $program == agree || $program == relay ]] && cap=99
launch="$root/src/backends/$b/launch"
export RUSTFLAGS='--cfg trame_checked'
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/$b}"
cargo build -q --no-default-features --features "$b" --example "$program"
prog="$CARGO_TARGET_DIR/debug/examples/$program"
fail=0
# shape NAME HOSTS-OPTIONS... : the --workers/env the launch gets, the host count, the counts the hosts declare.
run() {
  local name="$1" split="$2" counts="$3" how="$4"
  local d; d="$(mktemp -d)"
  local rc=0
  if [[ $how == opt ]]; then
    "$launch" --split "$split" --workers "$counts" --timeout 120 --out "$d" -- "$prog" >/dev/null 2>"$d/launch.err" || rc=$?
  else
    TRAME_WORKERS="$counts" "$launch" --split "$split" --timeout 120 --out "$d" -- "$prog" >/dev/null 2>"$d/launch.err" || rc=$?
  fi
  if [[ $rc -eq 2 && ! -f $d/host0.out ]]; then echo "skip $b $program $name: the backend refuses $split hosts"; rm -rf "$d"; return; fi
  local want="" got="" h=0 ranks=""
  for ((h = 0; h < split; h++)); do want+="host $h: ok"$'\n'; done
  got="$(cat "$d/host0.out" 2>/dev/null || true)"$'\n'
  [[ $rc -eq 0 && $got == "$want" ]] || { echo "FAIL $b $program $name: exit $rc, host0 printed:"; cat "$d/host0.out" "$d/host0.err" "$d/launch.err"; fail=1; rm -rf "$d"; return; }
  # Ranks per host: the workers each host declared (at most 6) and its leader.
  ranks="$(awk -F'\t' 'NR > 1 { n[$1]++ } END { for (h = 0; h in n; h++) printf "%s%d", (h ? "," : ""), n[h] }' "$d/cpu.tsv")"
  local expect=""
  IFS=, read -r -a cs <<<"$counts"
  for c in "${cs[@]}"; do expect+="${expect:+,}$(( (c > cap ? cap : c) + 1 ))"; done
  # A backend whose host is one process (one row per host in cpu.tsv) has no ranks to count.
  if [[ $(awk 'NR > 1' "$d/cpu.tsv" | wc -l) -eq $split ]]; then echo "ok   $b $program $name: workers $counts (no ranks to count)"; rm -rf "$d"; return; fi
  [[ $ranks == "$expect" ]] || { echo "FAIL $b $program $name: ranks per host $ranks, want $expect"; fail=1; rm -rf "$d"; return; }
  echo "ok   $b $program $name: workers $counts, ranks per host $ranks"
  rm -rf "$d"
}
run one-host 1 4 opt
run three-hosts 3 3,1,2 opt
run two-hosts 2 1,4 opt
run two-hosts-env 2 2,5 env
run capped 2 9,2 opt
exit $fail
