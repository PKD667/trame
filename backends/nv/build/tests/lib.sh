#!/bin/sh
# Shared local preflight for the pinned NV build campaign.

: "${NVMPI_CAMPAIGN:?NVMPI_CAMPAIGN must name the approved campaign stamp}"
: "${NVMPI_BASE:?NVMPI_BASE must name the approved campaign directory}"
: "${NVMPI_CUDA_OXIDE:?NVMPI_CUDA_OXIDE must name the approved backend checkout}"
: "${NVMPI_RUSTUP_HOME:?NVMPI_RUSTUP_HOME must name the campaign Rustup home}"
: "${NVMPI_CARGO_HOME:?NVMPI_CARGO_HOME must name the campaign Cargo home}"
: "${NVMPI_TARGET_DIR:?NVMPI_TARGET_DIR must name the campaign target}"
: "${NVMPI_PREFIX:?NVMPI_PREFIX must name the campaign dependency prefix}"
: "${RUSTUP_TOOLCHAIN:?RUSTUP_TOOLCHAIN must be selected explicitly}"
[ "$RUSTUP_TOOLCHAIN" = nightly-2026-04-03 ] || {
	echo "[preflight] RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN; expected nightly-2026-04-03" >&2
	exit 1
}
case "$NVMPI_CAMPAIGN" in *[!A-Za-z0-9._-]*|'') echo "[preflight] invalid campaign stamp: $NVMPI_CAMPAIGN" >&2; exit 1 ;; esac
case "$NVMPI_BASE" in /*) ;; *) echo "[preflight] NVMPI_BASE must be absolute: $NVMPI_BASE" >&2; exit 1 ;; esac
for path in "$NVMPI_CUDA_OXIDE" "$NVMPI_RUSTUP_HOME" "$NVMPI_CARGO_HOME" "$NVMPI_TARGET_DIR" "$NVMPI_PREFIX"; do
	case "$path" in
		"$NVMPI_BASE"/*) ;;
		*) echo "[preflight] path is outside campaign $NVMPI_CAMPAIGN: $path" >&2; exit 1 ;;
	esac
done
case "$NVMPI_BASE" in */nv-campaigns/"$NVMPI_CAMPAIGN") ;; *)
	echo "[preflight] NVMPI_BASE does not identify campaign $NVMPI_CAMPAIGN: $NVMPI_BASE" >&2
	exit 1
;; esac
[ -z "${IN_NIX:-}" ] || { echo "[preflight] IN_NIX is set; plain shell required" >&2; exit 1; }
export RUSTUP_HOME="$NVMPI_RUSTUP_HOME" CARGO_HOME="$NVMPI_CARGO_HOME"
export NVMPI_PROJECT="$NVMPI_BASE/src" CARGO_TARGET_DIR="$NVMPI_TARGET_DIR"
export PATH="$CARGO_HOME/bin:$NVMPI_TARGET_DIR/release:$PATH"

command -v rustup >/dev/null 2>&1 || { echo "[preflight] rustup executable missing" >&2; exit 1; }
toolchain="$RUSTUP_HOME/toolchains/$RUSTUP_TOOLCHAIN-x86_64-unknown-linux-gnu"
[ -d "$toolchain" ] || { echo "[preflight] installed toolchain directory missing: $toolchain" >&2; exit 1; }
rustc_path="$toolchain/bin/rustc"
cargo_path="$toolchain/bin/cargo"
[ -x "$rustc_path" ] || { echo "[preflight] installed rustc is not executable: $rustc_path" >&2; exit 1; }
[ -x "$cargo_path" ] || { echo "[preflight] installed cargo is not executable: $cargo_path" >&2; exit 1; }
export RUSTC="$rustc_path"
rustc_info=$("$RUSTC" -Vv) || { echo "[preflight] rustc -Vv failed: $RUSTC" >&2; exit 1; }
"$cargo_path" -V >/dev/null || { echo "[preflight] cargo -V failed: $cargo_path" >&2; exit 1; }
PATH="$toolchain/bin:$PATH"
export PATH
release=$(printf '%s\n' "$rustc_info" | awk '/^release:/ {print; exit}')
commit=$(printf '%s\n' "$rustc_info" | awk '/^commit-hash:/ {print; exit}')
case "$release" in 'release: 1.'*-nightly) ;; *) echo "[preflight] unexpected rustc $release" >&2; exit 1 ;; esac
[ "$commit" = 'commit-hash: 55e86c996809902e8bbad512cfb4d2c18be446d9' ] || { echo "[preflight] rustc commit does not match pinned nightly 55e86c996809902e8bbad512cfb4d2c18be446d9: $commit" >&2; exit 1; }

: "${CUDA_HOME:?CUDA_HOME must be the approved toolkit root}"
: "${NVMPI_NVVM_DIR:?NVMPI_NVVM_DIR must be the approved NVVM directory}"
: "${LIBCLANG_PATH:?LIBCLANG_PATH must be the approved libclang directory}"
[ -f "$CUDA_HOME/include/cuda.h" ] || { echo "[preflight] missing toolkit header: $CUDA_HOME/include/cuda.h" >&2; exit 1; }
[ -e "$NVMPI_NVVM_DIR/libnvvm.so" ] || { echo "[preflight] missing NVVM library: $NVMPI_NVVM_DIR/libnvvm.so" >&2; exit 1; }
[ -e "$LIBCLANG_PATH/libclang-19.so" ] || { echo "[preflight] missing pinned libclang-19.so: $LIBCLANG_PATH/libclang-19.so" >&2; exit 1; }
export CUDA_TOOLKIT_PATH="$CUDA_HOME"

require_sm70() {
	cc=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader | head -1)
	[ "${cc%.*}" -ge 7 ] || { echo "[$1] device-scope acquire/release requires sm_70+"; exit 1; }
}

run() {
	[ -z "${IN_NIX:-}" ] || { echo "[preflight] IN_NIX is set; refusing command" >&2; return 1; }
	"$@"
}
