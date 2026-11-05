#!/usr/bin/env bash
# Runs every MPI check on this machine. Run it inside the Nix shell:
#   nix develop .#mpi --command backends/mpi/tests/run.sh
# Exits non-zero on the first failing check. Output: $OUT (default: a fresh temp dir).
# Each check with W workers runs with W + 1 ranks: the leader is the last rank.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$root/backends/mpi"
export CARGO_TARGET_DIR="$root/target/mpi"
# A build that fails stops the script: a stale binary would otherwise pass the checks.
build_or_die() {
  local out
  if ! out="$(cargo build -q "$@" 2>&1)"; then echo "$out" >&2; echo "FAIL build: cargo build -q $*" >&2; exit 1; fi
  grep -v 'Git tree' <<<"$out" >&2 || true
}
build_or_die --no-default-features --features mpi --bins
# The latency timings are a release-build claim (record/README.md), so the latency check runs
# the release binary. The rest are checks, run in the debug build with overflow checks on.
build_or_die --no-default-features --features mpi --release --bin latency
B="$CARGO_TARGET_DIR/debug"
R="$CARGO_TARGET_DIR/release"
OUT="${OUT:-$(mktemp -d)}"
mkdir -p "$OUT"
MPI=(mpirun --oversubscribe)
T=120

# The filter drops a known Open MPI component notice; `|| true` keeps an empty result from failing
# under pipefail. The exit status is mpirun's.
run() {
  local rc=0
  timeout "$T" "${MPI[@]}" "$@" >"$OUT/.last" 2>&1 || rc=$?
  grep -v 'osc_ucx\|^ *$' "$OUT/.last" || true
  return "$rc"
}

for w in 3 4 6; do
  for s in 1 2 3 4 5; do
    d="$OUT/fifo-$w-$s"
    run -n $((w + 1)) "$B/check" fifo --seed "$s" --dir "$d" >/dev/null
    "$B/check" verify "$d"
  done
done

run -n 3 "$B/check" overwrite --seed 7 --cap 4 --msgs 20000
run -n 3 "$B/check" overwrite --seed 7 --cap 4 --msgs 20000 --late
run -n 4 "$B/check" fair --msgs 16
run -n 3 "$B/check" bigroute

# Latency record: N = 6636 round trips between two workers, leader on rank 2, written to
# $OUT/latency. See RESULT.md for the run that is kept.
run -n 3 "$R/latency" --samples 6636 --max-gap-ns 500000 --seed 20261026 --dir "$OUT/latency"
# Every object a run makes is unlinked when the run ends (object.rs). A leftover is a leak.
leftover="$(ls /dev/shm 2>/dev/null | grep '^trame-' || true)"
if [[ -n "$leftover" ]]; then
  echo "trame objects left in /dev/shm:" >&2
  echo "$leftover" >&2
  exit 1
fi
echo "all checks passed; outputs in $OUT"
