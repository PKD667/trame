#!/bin/sh
# Run a test on an explicitly approved campaign destination.
# Usage: NVMPI_CAMPAIGN=<stamp> NVMPI_BASE='~/nv-campaigns/<stamp>' \
#   trame/backends/nv/build/remote.sh <test> <user@host>
# One manifest-frozen workspace snapshot is synced, then prep + test run from that snapshot.
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
#     trame/backends/nv/build/remote.sh cases pkronlun@graffiti-3.nancy.grid5000.fr
# The same with one GPU of a shared node:
#   NVMPI_JUMP=nancy.g5k NVMPI_OAR_JOB=6939656 NVMPI_MODULES=cuda-toolkit/12.9.1 \
#     NVMPI_TARGET_DIR=/tmp/nvmpi-target trame/backends/nv/build/remote.sh cases graffiti-3
set -eu

TEST=${1:?usage: remote.sh <test> <user@host>}
HOST=${2:?remote host must be explicit}
CAMPAIGN=${NVMPI_CAMPAIGN:?NVMPI_CAMPAIGN must name the approved campaign stamp}
BASE=${NVMPI_BASE:?NVMPI_BASE must name the approved campaign base}
case "$CAMPAIGN" in *[!A-Za-z0-9._-]*|'') echo "[remote] invalid campaign stamp: $CAMPAIGN"; exit 1 ;; esac
case "$BASE" in "~/nv-campaigns/$CAMPAIGN") ;; *) echo "[remote] NVMPI_BASE must be ~/nv-campaigns/$CAMPAIGN, got: $BASE"; exit 1 ;; esac
BASE="\$HOME/nv-campaigns/$CAMPAIGN"

here=$(CDPATH= cd "$(dirname "$0")" && pwd -P)
root=$(CDPATH= cd "$here/../../.." && pwd -P)
MANIFEST_TOOL="$root/build/manifest.py"
LOCAL_EVIDENCE=${NVMPI_MANIFEST_DIR:?NVMPI_MANIFEST_DIR must name the local campaign manifest directory}
FROZEN="$LOCAL_EVIDENCE/source"
[ -f "$LOCAL_EVIDENCE/manifest.json" ] || { echo "[remote] local source manifest missing: $LOCAL_EVIDENCE/manifest.json" >&2; exit 1; }
[ -f "$FROZEN/trame/backends/nv/build/tests/$TEST.sh" ] || { echo "[remote] no such test in frozen source: $TEST"; exit 1; }
python3 "$MANIFEST_TOOL" verify "$LOCAL_EVIDENCE/manifest.json" "$FROZEN"
SOURCE=$(sha256sum "$LOCAL_EVIDENCE/manifest.json" | awk '{print $1}')
echo "[remote] frozen source S=$SOURCE"

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

remote "if [ -e \"$BASE/src\" ] || [ -e \"$BASE/evidence\" ]; then echo '[remote] campaign destination already exists: $BASE' >&2; exit 1; fi; mkdir -p \"$BASE/src\" \"$BASE/evidence\""

# The manifest is the file list: archive no files that were not frozen, and omit no frozen input.
echo "[remote] syncing manifest-frozen workspace to $HOST:$BASE"
tar -C "$LOCAL_EVIDENCE" -cf - manifest.json | remote "tar -xf - -C \"$BASE/evidence\""
python3 -c 'import json, os, sys; files=json.load(open(sys.argv[1], encoding="utf-8"))["files"]; sys.stdout.buffer.write(b"".join(os.fsencode(path)+b"\0" for path in files))' \
	"$LOCAL_EVIDENCE/manifest.json" |
	tar --null -C "$FROZEN" -T - -cf - |
	remote "tar -xf - -C \"$BASE/src\""
python3 "$MANIFEST_TOOL" verify "$LOCAL_EVIDENCE/manifest.json" "$FROZEN"
remote "python3 \"$BASE/src/build/manifest.py\" verify \"$BASE/evidence/manifest.json\" \"$BASE/src\""

# Forward NVMPI_* overrides (home relocation, device selection) and the measurement's arguments.
fwd=""
for v in $(env | sed -n 's/^\(NVMPI_[A-Z0-9_]*\|CUDA_HOME\|LIBCLANG_PATH\|TRAME_MEASURE\)=.*/\1/p'); do
	[ "$v" = NVMPI_BASE ] && continue
	[ "$v" = NVMPI_MANIFEST_DIR ] && continue
	fwd="$fwd export $v=$(q "$(printenv "$v")");"
done

remote_dir="$BASE/evidence"
pull_evidence() {
	phase=$1
	dest="$LOCAL_EVIDENCE/remote-evidence/$phase"
	[ ! -e "$dest" ] || { echo "[remote] local evidence phase already exists: $dest" >&2; exit 1; }
	mkdir -p "$dest"
	remote "tar -C \"$remote_dir\" -cf - ." | tar -xf - -C "$dest"
	remote_hash=$(remote "cd \"$remote_dir\" && for f in prep-evidence.txt cargo-nv-identity.log; do if [ -f \"\$f\" ]; then sha256sum \"\$f\" || exit; fi; done")
	local_hash=$(cd "$dest" && for f in prep-evidence.txt cargo-nv-identity.log; do if [ -f "$f" ]; then sha256sum "$f" || exit; fi; done)
	[ "$remote_hash" = "$local_hash" ] || { echo "[remote] evidence hash mismatch after $phase" >&2; exit 1; }
}
run_phase() {
	phase=$1
	command=$2
	if remote "$command"; then status=0; else status=$?; fi
	pull_evidence "$phase"
	[ "$status" -eq 0 ] || exit "$status"
}

prep_cmd="RUSTUP_TOOLCHAIN=nightly-2026-04-03; export RUSTUP_TOOLCHAIN; NVMPI_BASE=\"$BASE\"; NVMPI_MANIFEST_DIR=\"$remote_dir\"; export NVMPI_BASE NVMPI_MANIFEST_DIR; cd \"$BASE/src\" && $fwd sh trame/backends/nv/build/tests/prep.sh"
test_cmd="RUSTUP_TOOLCHAIN=nightly-2026-04-03; export RUSTUP_TOOLCHAIN; NVMPI_BASE=\"$BASE\"; NVMPI_MANIFEST_DIR=\"$remote_dir\"; export NVMPI_BASE NVMPI_MANIFEST_DIR; cd \"$BASE/src\" && $fwd sh trame/backends/nv/build/tests/$TEST.sh"
if [ -n "${NVMPI_MODULES:-}" ]; then
	prep_cmd="bash -lc $(q "module load $NVMPI_MODULES && $prep_cmd")"
	test_cmd="bash -lc $(q "module load $NVMPI_MODULES && $test_cmd")"
fi
echo "[remote] prep on $HOST"
run_phase prep "$prep_cmd"
echo "[remote] $TEST on $HOST"
run_phase test "$test_cmd"
# Evidence retrieval and checksum comparison complete the phase gate.
