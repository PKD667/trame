#!/bin/sh
# Transport refusals on a device: a short buffer, a full lane, a peer that never runs; and entry
# through the launch description the launcher writes.
set -eu
. "$(dirname "$0")/lib.sh"

export CUDA_OXIDE_BACKEND="$CARGO_HOME/cuda-oxide/librustc_codegen_cuda.so"

cc=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader | head -1)
[ "${cc%.*}" -ge 7 ] || { echo "[cases] device-scope acquire/release requires sm_70+"; exit 1; }

cd "$NVMPI_PROJECT"
run cargo nv test -p trame --features cuda --lib -- --ignored --exact --nocapture nv::measure::cases
