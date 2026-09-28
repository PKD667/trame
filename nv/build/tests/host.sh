#!/bin/sh
# trame's host-side tests: the backend that does nothing, nv's host model, and the conformance
# claims on nv's host model, the claims and M5 as separate invocations.
set -eu
. "$(dirname "$0")/lib.sh"

cd "$NVMPI_PROJECT"
run cargo test -p trame
run cargo test -p trame --features nv
for launch in claims pressure; do
	run cargo test -p trame --features nv --lib "nv::tests::conformance::$launch" -- --ignored --exact --nocapture
done
