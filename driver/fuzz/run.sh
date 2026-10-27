#!/usr/bin/env bash
# Usage: driver/fuzz/run.sh <seed> <batches> <K> [--extra]
#
# Builds the driver and the fuzzer with nix, runs <batches> batches of K programs from <seed>, and
# prints the counts (driver/fuzz/README.md). Exit status: 0 agreement, 1 any mismatch, 2 harness.
# Environment: TRAME_DRIVER_BIN, if set, is the driver's bin directory (skips the driver build).
set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage: $0 <seed> <batches> <K> [--extra]" >&2
  exit 2
fi
seed=$1
batches=$2
k=$3
shift 3
extra=("$@")

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
export RUSTUP_TOOLCHAIN=nightly-2026-04-21

if [ -n "${TRAME_DRIVER_BIN:-}" ]; then
  drv_bin=$TRAME_DRIVER_BIN
else
  # The driver package is an output of the repository's flake.nix. A `path:` flake copies the whole
  # repository, so a build directory that another process is writing can vanish during the copy;
  # retry rather than fail.
  drv=
  for attempt in 1 2 3; do
    if drv=$(nix build "path:$repo#trame-driver" --no-link --print-out-paths); then break; fi
    echo "driver build attempt $attempt failed; retrying" >&2
    sleep 20
  done
  [ -n "$drv" ] || { echo "driver build failed" >&2; exit 2; }
  drv_bin=$drv/bin
fi
echo "driver: $drv_bin"
"$drv_bin/trame-driver" -vV 2>/dev/null | head -1 | sed 's/^/driver version: /' || true

fuzz=$(nix build "path:$here" --no-link --print-out-paths)
echo "fuzzer: $fuzz"
echo "machine: $(uname -srm), $(nproc) cpus"

set +e
"$fuzz/bin/trame-fuzz" run --seed "$seed" --batches "$batches" --k "$k" "${extra[@]}" \
  --driver-bin "$drv_bin" --out "$here"
status=$?
set -e
exit $status
