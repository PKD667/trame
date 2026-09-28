#!/bin/sh
# A dispatch region partitioned across the 32 lanes of a real warp.
set -eu
. "$(dirname "$0")/lib.sh"

export CUDA_OXIDE_BACKEND="$CARGO_HOME/cuda-oxide/librustc_codegen_cuda.so"

cc=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader | head -1)
[ "${cc%.*}" -ge 7 ] || { echo "[declared] device-scope acquire/release requires sm_70+"; exit 1; }

cd "$NVMPI_PROJECT"
run cargo nv run -p trame --example nv-declared --features cuda
