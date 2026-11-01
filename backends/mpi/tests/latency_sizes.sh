#!/usr/bin/env bash
# Median and p99 round trip per probe size, for the p2p and rma small-frame paths (RMA.md), on this machine.
# Each cell is one run of `latency` with the same seed, so the paths see the same gap sequence.
#
#   nix develop .#mpi --command backends/mpi/tests/latency_sizes.sh [OUT.tsv]
#
# Two placements. `one-machine-split` is TRAME_MPI_NODE_RANKS=1: each worker is a node of its own, on one
# machine. A route between them is labelled node-split-one-machine (latency.rs placement): it is not a
# cross-node route, its frames cross the machine's memory. `same-node` is no split: the reference, with no
# relay on the route. The TSV has a '#' header with the command, Open MPI version and machine.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
export CARGO_TARGET_DIR="$root/target/mpi"
out="${1:-$root/backends/mpi/tests/latency-sizes.tsv}"
samples="${SAMPLES:-6636}"
reps="${REPS:-3}"
sizes="${SIZES:-8 64 256 1024 4096 8192 65536}"

if ! built="$(cargo build -q --no-default-features --features mpi --release --bin latency 2>&1)"; then
  echo "$built" >&2
  echo "FAIL build: latency" >&2
  exit 1
fi
R="$CARGO_TARGET_DIR/release"
mpi_version="$(mpirun --version | head -1)"
machine="$(uname -srm); $(lscpu | sed -n 's/^Model name: *//p'); $(nproc) CPUs"
work="$(mktemp -d)"

{
  echo "# latency sizes: median and p99 round trip per probe size, p2p vs rma (RMA.md)."
  echo "# command: [TRAME_MPI_NODE_RANKS=1|unset] TRAME_MPI_SMALL=<p2p|rma> mpirun --oversubscribe -n 3 latency --samples $samples --bytes <size> --seed 20261026 --dir <tmp>"
  echo "# seed 20261026, $samples samples per run, $reps runs per cell; values in ns on A's clock."
  echo "# $mpi_version"
  echo "# machine: $machine"
  echo "# one-machine-split: TRAME_MPI_NODE_RANKS=1 (two nodes of one rank each, one machine). Not cross-node."
  echo "# same-node: no split. The reference for the relay's cost."
  printf 'bytes\tplacement\tpath\trep\troute\tn\tmin_ns\tmedian_ns\tp99_ns\tmax_ns\n'
} >"$out"

for b in $sizes; do
  for placement in one-machine-split same-node; do
    for path in p2p rma; do
      for rep in $(seq 1 "$reps"); do
        dir="$work/$placement-$path-$b-$rep"
        # `env -u` leaves TRAME_MPI_NODE_RANKS unset for the same-node reference (nodes.rs).
        if [[ $placement == same-node ]]; then k=(env -u TRAME_MPI_NODE_RANKS); else k=(env TRAME_MPI_NODE_RANKS=1); fi
        line="$("${k[@]}" TRAME_MPI_SMALL="$path" timeout 120 mpirun --oversubscribe -n 3 "$R/latency" \
          --samples "$samples" --bytes "$b" --seed 20261026 --dir "$dir" | grep '^summary' || true)"
        if [[ -z "$line" ]]; then echo "FAILED: bytes=$b $placement path=$path rep=$rep" >&2; exit 1; fi
        # summary: bytes, route, n, min, median, p99, max
        IFS=$'\t' read -r _ sb route n mn med p99 mx <<<"$line"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$sb" "$placement" "$path" "$rep" "$route" "$n" "$mn" "$med" "$p99" "$mx" >>"$out"
      done
    done
  done
done
rm -rf "$work"
echo "wrote $out"
