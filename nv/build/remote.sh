#!/bin/sh
# Run a test on a remote host. Usage: trame/nv/build/remote.sh <test> [user@host]
# The workspace and test scripts are synced, then prep + test run.
# NVMPI_BASE relocates everything on the far side (quota-cramped homes),
# e.g. NVMPI_BASE=/local/pkronlun on g5k. All NVMPI_* overrides are
# forwarded into the remote prep + test.
set -eu

TEST=$1
HOST=${2:-${NVMPI_HOST:-pkd@camarade.unsuspicious.org}}
BASE=${NVMPI_BASE:-$HOME/nvmpi-test}

here=$(CDPATH= cd "$(dirname "$0")" && pwd -P)
root=$(CDPATH= cd "$here/../../.." && pwd -P)
[ -f "$here/tests/$TEST.sh" ] || { echo "[remote] no such test: $TEST"; exit 1; }

ssh "$HOST" "mkdir -p \"$BASE\"/snnus \"$BASE\"/scripts"

# The workspace's manifests and every member's sources: cargo loads the whole workspace to build
# one package of it.
echo "[remote] syncing workspace to $HOST:$BASE"
tar -C "$root" --exclude=target --exclude=__pycache__ \
	-cf - Cargo.toml Cargo.lock rust-toolchain.toml backend mpi-rma src/nerve src/ffi experiments |
	ssh "$HOST" "tar -xf - -C \"$BASE\"/snnus"

echo "[remote] syncing scripts"
for f in lib.sh prep.sh "$TEST.sh"; do
	scp -q "$here/tests/$f" "$HOST:$BASE/scripts/$f"
done

# Forward NVMPI_* overrides (home relocation, device selection) to the host.
q() { printf "'%s'" "$(printf %s "$1" | sed "s/'/'\\\\''/g")"; }
fwd=""
for v in $(env | sed -n 's/^\(NVMPI_[A-Z0-9_]*\)=.*/\1/p'); do
	fwd="$fwd export $v=$(q "$(printenv "$v")");"
done

echo "[remote] prep + $TEST on $HOST"
ssh "$HOST" "cd \"$BASE\" && $fwd sh scripts/prep.sh && sh scripts/$TEST.sh"

# The lockfile pins the cuda-oxide git rev; keep the repo canonical.
scp -q "$HOST:$BASE/snnus/Cargo.lock" "$root/Cargo.lock" 2>/dev/null || true