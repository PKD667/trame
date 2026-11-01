#!/usr/bin/env bash
# Runs every MPI check on this machine. Run it inside the Nix shell:
#   nix develop .#mpi --command backends/mpi/tests/run.sh
# Exits non-zero on the first failing check. Output: $OUT (default: a fresh temp dir).
# Each check with W workers runs with W + 1 ranks: the leader is the last rank.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$root/backends/mpi"
export CARGO_TARGET_DIR="$root/target/mpi"
# Every check runs with each small-frame path (RMA.md): TRAME_MPI_SMALL=p2p, then =rma, each in its own
# subdirectory of $OUT. The inner run is this script again, marked by TRAME_MPI_SMALL_INNER.
if [[ -z "${TRAME_MPI_SMALL_INNER:-}" ]]; then
  top="${OUT:-$(mktemp -d)}"
  rc=0
  for m in p2p rma; do
    mkdir -p "$top/$m"
    TRAME_MPI_SMALL_INNER=1 TRAME_MPI_SMALL="$m" OUT="$top/$m" "$root/backends/mpi/tests/run.sh" || { rc=1; echo "FAILED: TRAME_MPI_SMALL=$m" >&2; }
  done
  if ((rc)); then echo "checks failed in at least one small-frame path; outputs in $top" >&2; fi
  exit "$rc"
fi
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

# A check run under `check` is named if it fails, and the rest still run; the script exits non-zero at the
# end (the multi-node section and the refusal checks share the list).
nodes_failed=()
check() {
  local name=$1; shift
  if ! "$@"; then nodes_failed+=("$name"); echo "FAILED: $name" >&2; fi
}

for w in 3 4 6; do
  for s in 1 2 3 4 5; do
    d="$OUT/fifo-$w-$s"
    run -n $((w + 1)) "$B/check" fifo --seed "$s" --dir "$d" >/dev/null
    "$B/check" verify "$d"
  done
done

# TRAME_MPI_SMALL must be p2p or rma, and anything else is refused on every rank (RMA.md, Setting).
refused_small() { # value
  local rc=0 out
  out="$(env TRAME_MPI_SMALL="$1" timeout "$T" mpirun --oversubscribe -n 3 "$B/check" fifo --seed 1 --dir "$OUT/refused" 2>&1)" || rc=$?
  [[ $rc -ne 0 ]] && grep -qF "TRAME_MPI_SMALL is not p2p or rma: \"$1\"" <<<"$out"
}
check "small: TRAME_MPI_SMALL=bogus refused" refused_small bogus
check "small: TRAME_MPI_SMALL= (empty) refused" refused_small ""
run -n 3 "$B/check" overwrite --seed 7 --cap 4 --msgs 20000
run -n 3 "$B/check" overwrite --seed 7 --cap 4 --msgs 20000 --late
run -n 4 "$B/check" fair --msgs 16
run -n 6 "$B/check" barrier --seed 3 --rounds 300
run -n 5 "$B/check" deadlock
run -n 3 "$B/check" publication
run -n 3 "$B/check" small --msgs 200

# Latency record: N = 6636 round trips between two workers, leader on rank 2, written to
# $OUT/latency. See RESULT.md for the run that is kept.
run -n 3 "$R/latency" --samples 6636 --max-gap-ns 500000 --seed 20261026 --dir "$OUT/latency"
# Several nodes on one machine (NODES.md, Testing on one machine): the same checks with every node cut
# into groups of k ranks. Each check is run under `if`, so a failure is named and the rest still run;
# the script exits non-zero at the end. Output for every run is in $OUT.
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
  check "k=$k publication" run -n 3 "$B/check" publication
  check "k=$k small" run -n 3 "$B/check" small --msgs 200
  check "k=$k latency" run -n 3 "$R/latency" --samples 6636 --max-gap-ns 500000 --seed 20261026 --dir "$OUT/k$k-latency"
  unset TRAME_MPI_NODE_RANKS
}
multi 1
multi 2
if ((${#nodes_failed[@]})); then
  echo "checks failed: ${nodes_failed[*]}; outputs in $OUT" >&2
  exit 1
fi
# Every object a run makes is unlinked when the run ends (object.rs, publish.rs). A leftover is a leak.
leftover="$(ls /dev/shm 2>/dev/null | grep '^trame-' || true)"
if [[ -n "$leftover" ]]; then
  echo "trame objects left in /dev/shm:" >&2
  echo "$leftover" >&2
  exit 1
fi
echo "all checks passed (TRAME_MPI_SMALL=$TRAME_MPI_SMALL); outputs in $OUT"
