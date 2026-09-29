#!/bin/sh
# Exercise and measure a backend, with no NERVE in the measurement.
#
# Everything here runs the backend crate's own executables, so what comes out is the wire's cost
# and nothing else. The runtime's numbers are scripts/analysis/, and they are a different question.
#
#     trame/bench.sh                       # the mpi build, 2 ranks
#     trame/bench.sh rma 4                 # the acknowledged ring, 4 ranks
#     trame/bench.sh rma-lossy 4
#     trame/bench.sh mpi 4 particles       # one named experiment
#
# `pingpong` times a round trip across message sizes. `verifiable` relaxes a grid and checks the
# answer against the serial one, so it says whether the transport is correct as well as how fast.
# `traffic` runs eight named patterns, from uniform through incast to request/response.
# `particles` needs a square rank count and is not in the default set for that reason.
#
# Never oversubscribe: pick a worker count below the core count; the leader takes one more.
set -eu
cd "$(CDPATH= cd "$(dirname "$0")" && pwd)/.."

BACKEND=${1:-mpi}
RANKS=${2:-2}
EXPERIMENTS=${3:-"pingpong verifiable traffic"}

# The backend says how to build for it and what launches it, so this file knows no backend by name.
ENV=$(grep -lx "NAME=$BACKEND" trame/*/build.env 2>/dev/null | head -1)
if [ -z "$ENV" ]; then
	echo "unknown backend: $BACKEND (no trame/*/build.env declares it)" >&2
	echo "known: $(grep -h '^NAME=' trame/*/build.env 2>/dev/null | sed 's/^NAME=//' | tr '\n' ' ')" >&2
	exit 2
fi
# shellcheck source=/dev/null
. "./$ENV"

CORES=$(nproc 2>/dev/null || echo 1)
if [ $((RANKS + 1)) -gt "$CORES" ]; then
	echo "refusing $RANKS workers and a leader on $CORES cores: oversubscribed MPI measures the scheduler" >&2
	exit 2
fi

# shellcheck disable=SC2086
for name in $EXPERIMENTS; do
	cargo build --release -p trame --features "$FEATURES" --example "$name" >&2
	echo "=== $name: $BACKEND, $RANKS ranks" >&2
	TRAME_WORKERS=$RANKS "$LAUNCH" "$LAUNCH_RANKS" "$RANKS" "target/release/examples/$name" : "$LAUNCH_RANKS" 1 "target/release/examples/$name" --leader
done
