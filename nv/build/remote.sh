#!/bin/sh
# Run a test on a remote host. Usage: trame/nv/build/remote.sh <test> [user@host]
# The workspace and test scripts are synced, then prep + test run.
# NVMPI_BASE relocates everything on the far side (quota-cramped homes),
# e.g. NVMPI_BASE=/local/pkronlun on g5k. All NVMPI_* overrides and TRAME_MEASURE (the
# measurement's arguments) are forwarded into the remote prep + test.
#
# NVMPI_JUMP is an ssh jump host, for compute nodes reachable only through a frontend.
# NVMPI_MODULES names Lmod modules loaded in a login shell before prep, for hosts whose CUDA
# toolkit is a module rather than a system package; lib.sh finds the toolkit from its `nvcc`.
#
# NVMPI_OAR_JOB is the OAR job holding part of a node. OAR refuses plain ssh to a node the job does
# not hold whole, so every command then runs as `oarsh <node>` on the frontend NVMPI_JUMP, and the
# host is the bare node name.
#
# Grid'5000 Nancy, a GPU node held by an OAR job (home is NFS-shared, so the frontend's
# toolchain and cuda-oxide backend are the node's; the target dir goes to node-local /tmp):
#   NVMPI_JUMP=nancy.g5k NVMPI_MODULES=cuda-toolkit/12.9.1 NVMPI_TARGET_DIR=/tmp/nvmpi-target \
#     trame/nv/build/remote.sh cases pkronlun@graffiti-3.nancy.grid5000.fr
# The same with one GPU of a shared node:
#   NVMPI_JUMP=nancy.g5k NVMPI_OAR_JOB=6939656 NVMPI_MODULES=cuda-toolkit/12.9.1 \
#     NVMPI_TARGET_DIR=/tmp/nvmpi-target trame/nv/build/remote.sh cases graffiti-3
set -eu

TEST=$1
HOST=${2:-${NVMPI_HOST:-pkd@camarade.unsuspicious.org}}
BASE=${NVMPI_BASE:-\$HOME/nvmpi-test}

here=$(CDPATH= cd "$(dirname "$0")" && pwd -P)
root=$(CDPATH= cd "$here/../../.." && pwd -P)
[ -f "$here/tests/$TEST.sh" ] || { echo "[remote] no such test: $TEST"; exit 1; }

q() { printf "'%s'" "$(printf %s "$1" | sed "s/'/'\\\\''/g")"; }

# Run `$1` on the target, stdin passed through.
if [ -n "${NVMPI_OAR_JOB:-}" ]; then
	printf %s "$NVMPI_OAR_JOB" | grep -qE '^[0-9]+$' || { echo "[remote] NVMPI_OAR_JOB is not a job id: $NVMPI_OAR_JOB"; exit 1; }
	[ -n "${NVMPI_JUMP:-}" ] || { echo "[remote] NVMPI_OAR_JOB needs NVMPI_JUMP, the frontend that runs oarsh"; exit 1; }
	node=${HOST#*@}
	remote() { ssh "$NVMPI_JUMP" "OAR_JOB_ID=$NVMPI_OAR_JOB oarsh $node $(q "$1")"; }
else
	jump=${NVMPI_JUMP:+-J $NVMPI_JUMP}
	remote() { ssh $jump "$HOST" "$1"; }
fi

remote "mkdir -p \"$BASE\"/nerve \"$BASE\"/scripts"

# The workspace's manifests and every member's sources: cargo loads the whole workspace to build
# one package of it.
echo "[remote] syncing workspace to $HOST:$BASE"
tar -C "$root" --exclude=target --exclude=__pycache__ \
	-cf - Cargo.toml Cargo.lock rust-toolchain.toml trame mpi-rma src/nerve src/ffi experiments |
	remote "tar -xf - -C \"$BASE\"/nerve"

echo "[remote] syncing scripts"
tar -C "$here/tests" -cf - lib.sh prep.sh "$TEST.sh" |
	remote "tar -xf - -C \"$BASE\"/scripts"

# Forward NVMPI_* overrides (home relocation, device selection) and the measurement's arguments.
fwd=""
for v in $(env | sed -n 's/^\(NVMPI_[A-Z0-9_]*\|TRAME_MEASURE\)=.*/\1/p'); do
	fwd="$fwd export $v=$(q "$(printenv "$v")");"
done

cmd="cd \"$BASE\" && $fwd sh scripts/prep.sh && sh scripts/$TEST.sh"
if [ -n "${NVMPI_MODULES:-}" ]; then
	cmd="bash -lc $(q "module load $NVMPI_MODULES && $cmd")"
fi
echo "[remote] prep + $TEST on $HOST"
remote "$cmd"

# The lockfile pins the cuda-oxide git rev; keep the repo canonical.
remote "cat \"$BASE\"/nerve/Cargo.lock" >"$root/Cargo.lock"
