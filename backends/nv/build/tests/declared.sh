#!/bin/sh
# A dispatch region partitioned across the 32 lanes of a real warp.
set -eu
. "$(dirname "$0")/lib.sh"

export CUDA_OXIDE_BACKEND="$CARGO_HOME/cuda-oxide/librustc_codegen_cuda.so"

require_sm70 declared

cd "$NVMPI_PROJECT"
run cargo nv run -p trame --example nv-declared --features cuda
