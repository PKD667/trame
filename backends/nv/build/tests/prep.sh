#!/bin/sh
# Prepare only the already provisioned, private campaign toolchain and backend.
set -eu
. "$(dirname "$0")/lib.sh"

rev=a105afd522b73a6712a802e30bd900cd04cc8019
: "${NVMPI_MANIFEST_DIR:?NVMPI_MANIFEST_DIR must name the external campaign manifest directory}"
[ -d "$NVMPI_MANIFEST_DIR" ] || { echo "[prep] campaign manifest directory missing: $NVMPI_MANIFEST_DIR" >&2; exit 1; }
evidence="$NVMPI_MANIFEST_DIR/prep-evidence.txt"
[ ! -e "$evidence" ] || { echo "[prep] preparation evidence already exists: $evidence" >&2; exit 1; }
export CARGO_NET_OFFLINE=true
lock="$NVMPI_BASE/.prep.lock"
mkdir "$lock" 2>/dev/null || { echo "[prep] campaign preparation lock already exists: $lock" >&2; exit 1; }
trap 'rmdir "$lock"' EXIT HUP INT TERM

command -v rustup >/dev/null 2>&1 || { echo "[prep] rustup missing from campaign PATH" >&2; exit 1; }
rustup toolchain list | grep -F 'nightly-2026-04-03' >/dev/null || {
	echo "[prep] pinned toolchain nightly-2026-04-03 is not installed in $RUSTUP_HOME" >&2
	exit 1
}
[ -x "$NVMPI_CARGO_HOME/bin/cargo-oxide" ] || { echo "[prep] campaign cargo-oxide missing: $NVMPI_CARGO_HOME/bin/cargo-oxide" >&2; exit 1; }
[ -f "$NVMPI_PROJECT/Cargo.lock" ] || { echo "[prep] workspace Cargo.lock missing: $NVMPI_PROJECT/Cargo.lock" >&2; exit 1; }
grep -F "$rev" "$NVMPI_PROJECT/Cargo.lock" >/dev/null || { echo "[prep] workspace lockfile does not pin cuda-oxide $rev" >&2; exit 1; }
[ -d "$NVMPI_CUDA_OXIDE/.git" ] || { echo "[prep] pinned cuda-oxide checkout missing: $NVMPI_CUDA_OXIDE" >&2; exit 1; }
actual_rev=$(git -C "$NVMPI_CUDA_OXIDE" rev-parse HEAD) || { echo "[prep] cannot read cuda-oxide HEAD" >&2; exit 1; }
[ "$actual_rev" = "$rev" ] || { echo "[prep] cuda-oxide HEAD $actual_rev; expected $rev" >&2; exit 1; }
status=$(git -C "$NVMPI_CUDA_OXIDE" status --porcelain) || { echo "[prep] cannot read cuda-oxide status" >&2; exit 1; }
[ -z "$status" ] || { echo "[prep] cuda-oxide checkout is dirty: $status" >&2; exit 1; }

[ -f "$CUDA_HOME/include/cuda.h" ] || { echo "[prep] missing approved toolkit header: $CUDA_HOME/include/cuda.h" >&2; exit 1; }
[ -e "$NVMPI_NVVM_DIR/libnvvm.so" ] || { echo "[prep] missing approved NVVM library: $NVMPI_NVVM_DIR/libnvvm.so" >&2; exit 1; }
[ -e "$LIBCLANG_PATH/libclang-19.so" ] || { echo "[prep] missing pinned libclang-19.so: $LIBCLANG_PATH/libclang-19.so" >&2; exit 1; }

cd "$NVMPI_CUDA_OXIDE"
echo "[prep] running pinned setup at $actual_rev with $RUSTUP_TOOLCHAIN"
run cargo oxide setup
cd "$NVMPI_PROJECT/trame/backends/nv/cargo"
run cargo build --locked --release

backend="$NVMPI_CARGO_HOME/cuda-oxide/librustc_codegen_cuda.so"
[ -f "$backend" ] && [ -r "$backend" ] || { echo "[prep] backend is not a readable regular file: $backend" >&2; exit 1; }
backend_hash=$(sha256sum "$backend" | awk '{print $1}')
cuda_oxide_hash=$(git -C "$NVMPI_CUDA_OXIDE" rev-parse 'HEAD^{tree}')
printf 'cuda-oxide-rev %s\ncuda-oxide-archive-sha256 %s\nbackend %s\nbackend-sha256 %s\n' \
	"$actual_rev" "$cuda_oxide_hash" "$backend" "$backend_hash" >"$evidence"
