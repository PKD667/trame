#!/bin/sh
# Two-warp transport conformance test.
set -eu
. "$(dirname "$0")/lib.sh"

export CUDA_OXIDE_BACKEND="$CARGO_HOME/cuda-oxide/librustc_codegen_cuda.so"

require_sm70 pingpong

cd "$NVMPI_PROJECT"
run cargo nv test -p trame --features cuda --lib -- --ignored --exact --nocapture nv::measure::pingpong
