#!/usr/bin/env bash
# Local scaling matrix (scale/README.md). Run inside the MPI shell:
#   nix develop .#mpi --command scale/run.sh OUTDIR
# Each trame program runs with W+1 ranks (the leader is the last), each baseline with W ranks, all on one
# node with --oversubscribe. Per run, OUTDIR gets:
#   <key>.<W>.jsonl            stdout: one JSON line per worker, then the outcomes line
#   cpu.<key>.<W>.<rank>.txt   GNU time per rank: user, system and elapsed seconds
#   <key>.<W>.log              stderr of mpirun (the OSC UCX warnings come from the MPI stack)
# Keys: barrier, idle, fanin, ring64, ring4096 (trame); mpi_barrier, mpi_ring64, mpi_ring4096 (baselines).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
out="${1:?usage: scale/run.sh OUTDIR}"
mkdir -p "$out/bin"

TIME="$(command -v time)" || { echo "GNU time is needed (flake: add pkgs.time to the mpi shell)" >&2; exit 2; }
"$TIME" --version 2>&1 | grep -q 'GNU Time' || { echo "$TIME is not GNU time" >&2; exit 2; }

# BIN set (the nixsci deployment mpi-scale): the binaries are prebuilt in $BIN (scale_*, mpi_barrier,
# mpi_ring), so nothing is built here. Otherwise build as below.
if [[ -n "${BIN:-}" ]]; then
  B="$BIN"
  base_barrier="$BIN/mpi_barrier"
  base_ring="$BIN/mpi_ring"
else
  # Same checked cfg and target directory as programs/run.sh (backends/mpi/README.md).
  export RUSTFLAGS='--cfg trame_checked'
  export CARGO_TARGET_DIR="$root/target/mpi"
  # A failed build stops the run: the examples left in target/ would be stale.
  if ! cargo build -q --release --no-default-features --features mpi --examples 2>"$out/build.log"; then
    grep -v 'Git tree' "$out/build.log" >&2
    echo "build failed; see $out/build.log" >&2
    exit 1
  fi
  for b in barrier ring; do
    mpicc -O2 -o "$out/bin/$b" "scale/baseline/$b.c"
  done
  B="$CARGO_TARGET_DIR/release/examples"
  base_barrier="$out/bin/barrier"
  base_ring="$out/bin/ring"
fi

# run KEY W NRANKS PROG ARGS...: one mpirun, stdout to KEY.W.jsonl, per-rank CPU in cpu.KEY.W.RANK.txt.
run() {
  local key=$1 w=$2 n=$3 prog=$4
  shift 4
  # The load average is recorded with each run: other work on the machine makes timings unusable.
  echo "run $key W=$w ranks=$n load=$(cut -d' ' -f1-3 /proc/loadavg)" >&2
  timeout 900 mpirun --oversubscribe -n "$n" sh -c \
    "exec '$TIME' -f '%U %S %e' -o '$out'/cpu.$key.$w.\$OMPI_COMM_WORLD_RANK.txt '$prog' $*" \
    > "$out/$key.$w.jsonl" 2> "$out/$key.$w.log"
}

for w in 2 4 8; do
  run barrier "$w" $((w + 1)) "$B/scale_barrier" "$w" 10000
  run idle "$w" $((w + 1)) "$B/scale_idle" "$w" 3
  run fanin "$w" $((w + 1)) "$B/scale_fanin" "$w" 10000
  run mpi_barrier "$w" "$w" "$base_barrier" "$w" 10000
done
for w in 3 4 8; do
  run ring64 "$w" $((w + 1)) "$B/scale_ring" "$w" 10000 64 8
  run ring4096 "$w" $((w + 1)) "$B/scale_ring" "$w" 10000 4096 8
  run mpi_ring64 "$w" "$w" "$base_ring" "$w" 10000 64
  run mpi_ring4096 "$w" "$w" "$base_ring" "$w" 10000 4096
done
echo "done: $out" >&2
