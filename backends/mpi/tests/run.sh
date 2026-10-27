#!/usr/bin/env bash
# Runs every MPI check on this machine. Run it inside the Nix shell:
#   nix develop .#mpi --command backends/mpi/tests/run.sh
# Exits non-zero on the first failing check. Output: $OUT (default: a fresh temp dir).
# Each check with W workers runs with W + 1 ranks: the leader is the last rank.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$root/backends/mpi"
export CARGO_TARGET_DIR="$root/target/mpi"
cargo build -q --no-default-features --features mpi --bins 2>&1 | grep -v 'Git tree' || true
# The latency timings are a release-build claim (record/README.md), so the latency check runs
# the release binary. The rest are checks, run in the debug build with overflow checks on.
cargo build -q --no-default-features --features mpi --release --bin latency 2>&1 | grep -v 'Git tree' || true
B="$CARGO_TARGET_DIR/debug"
R="$CARGO_TARGET_DIR/release"
OUT="${OUT:-$(mktemp -d)}"
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
run -n 6 "$B/check" barrier --seed 3 --rounds 300
run -n 5 "$B/check" deadlock

# Latency record: N = 6636 round trips between two workers, leader on rank 2, written to
# $OUT/latency. See RESULT.md for the run that is kept.
run -n 3 "$R/latency" --samples 6636 --max-gap-ns 500000 --seed 20261026 --dir "$OUT/latency"
# Several nodes on one machine (NODES.md, Testing on one machine): the same checks with every node cut
# into groups of k ranks. Each check is run under `if`, so a failure is named and the rest still run;
# the script exits non-zero at the end. Output for every run is in $OUT.
nodes_failed=()
check() {
  local name=$1; shift
  if ! "$@"; then nodes_failed+=("$name"); echo "FAILED: $name" >&2; fi
}
multi() {
  local k=$1
  export TRAME_MPI_NODE_RANKS=$k
  for s in 1 2; do
    d="$OUT/k$k-fifo-$s"
    check "k=$k fifo seed $s" run -n 4 "$B/check" fifo --seed "$s" --dir "$d"
    check "k=$k fifo seed $s verify" "$B/check" verify "$d"
  done
  check "k=$k overwrite" run -n 3 "$B/check" overwrite --seed 7 --cap 4 --msgs 20000
  check "k=$k overwrite --late" run -n 3 "$B/check" overwrite --seed 7 --cap 4 --msgs 20000 --late
  check "k=$k fair" run -n 4 "$B/check" fair --msgs 16
  check "k=$k barrier" run -n 6 "$B/check" barrier --seed 3 --rounds 300
  check "k=$k deadlock" run -n 5 "$B/check" deadlock
  check "k=$k latency" run -n 3 "$R/latency" --samples 6636 --max-gap-ns 500000 --seed 20261026 --dir "$OUT/k$k-latency"
  unset TRAME_MPI_NODE_RANKS
}
multi 1
multi 2
if ((${#nodes_failed[@]})); then
  echo "multi-node checks failed: ${nodes_failed[*]}; outputs in $OUT" >&2
  exit 1
fi
echo "all checks passed; outputs in $OUT"
