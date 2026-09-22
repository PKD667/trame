#!/bin/sh
# Ensure the target host can build and run the cuda-oxide stack: pinned
# toolchain, pinned cargo-oxide, pinned CUDA runtime pieces, the exact
# cuda-oxide backend pinned by Cargo.lock.
#
# Everything lands under the operator's control, no sudo, no package installs:
# rustup into $NVMPI_RUSTUP_HOME, the cargo cache into $NVMPI_CARGO_HOME, any
# fetched .debs extracted into $NVMPI_PREFIX. Override any of these to point
# at a roomy or pre-warmed filesystem.
#
# lib.sh already exported the detected CUDA_HOME/LD_LIBRARY_PATH/LIBCLANG_PATH;
# this script fetches whatever detection could not find.
set -eu
. "$(dirname "$0")/lib.sh"

rev=a105afd522b73a6712a802e30bd900cd04cc8019
nightly=nightly-2026-04-03

# ---- toolchain: rustup + nightly + codegen components, no sudo ----
if ! command -v rustup >/dev/null 2>&1; then
	echo "[prep] installing rustup"
	command -v curl >/dev/null 2>&1 || { echo "[prep] curl missing"; exit 1; }
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
		sh -s -- -y --profile minimal
fi
rustup toolchain install "$nightly" --profile minimal
rustup component add rust-src rustc-dev llvm-tools --toolchain "$nightly"

# cargo-oxide builds the backend on first use; pin the same rev as the lock.
if ! command -v cargo-oxide >/dev/null 2>&1; then
	echo "[prep] installing cargo-oxide"
	cargo +"$nightly" install --git https://github.com/NVlabs/cuda-oxide.git \
		--rev "$rev" cargo-oxide
fi

# ---- CUDA toolkit headers for the host bindings ----
if [ -z "${CUDA_HOME:-}" ]; then
	echo "[prep] fetching pinned CUDA headers (nvidia-cuda-dev=12.4.127~12.4.1-2)"
	mkdir -p "$NVMPI_PREFIX"
	apt-get download "nvidia-cuda-dev=12.4.127~12.4.1-2"
	dpkg -x nvidia-cuda-dev_*.deb "$NVMPI_PREFIX"
	rm -f nvidia-cuda-dev_*.deb
	export CUDA_HOME="$NVMPI_PREFIX/usr/lib/cuda" CUDA_TOOLKIT_PATH="$NVMPI_PREFIX/usr/lib/cuda"
fi

# ---- NVVM runtime for PTX finalization, and libclang for bindgen ----
if [ -z "${NVMPI_NVVM_DIR:-}" ]; then
	echo "[prep] fetching pinned NVVM runtime (libnvvm4=12.4.131~12.4.1-2, libnvjitlink12=12.4.127~12.4.1-2)"
	mkdir -p "$NVMPI_PREFIX"
	apt-get download "libnvvm4=12.4.131~12.4.1-2" "libnvjitlink12=12.4.127~12.4.1-2"
	for d in libnvvm4_*.deb libnvjitlink12_*.deb; do
		dpkg -x "$d" "$NVMPI_PREFIX"
	done
	rm -f libnvvm4_*.deb libnvjitlink12_*.deb
	export LD_LIBRARY_PATH="$NVMPI_PREFIX/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
if [ -z "${LIBCLANG_PATH:-}" ]; then
	# The runtime library is fetched with the headers: `-dev` ships the `.so` symlink and the
	# runtime ships what it points at, and bindgen needs the target.
	echo "[prep] fetching pinned libclang (latest from the configured suite)"
	apt-get download libclang-19-dev libclang-common-19-dev libclang1-19
	for d in libclang-19-dev_*.deb libclang-common-19-dev_*.deb libclang1-19_*.deb; do
		dpkg -x "$d" "$NVMPI_PREFIX" 2>/dev/null || true
	done
	rm -f libclang-19-dev_*.deb libclang-common-19-dev_*.deb
	export LIBCLANG_PATH="$NVMPI_PREFIX/usr/lib/x86_64-linux-gnu"
fi

command -v git >/dev/null 2>&1 || { echo "[prep] git missing"; exit 1; }

mkdir -p "$NVMPI_CUDA_OXIDE"
if [ ! -d "$NVMPI_CUDA_OXIDE/.git" ]; then
	echo "[prep] cloning cuda-oxide"
	git clone --depth 1 https://github.com/NVlabs/cuda-oxide.git "$NVMPI_CUDA_OXIDE"
fi

cd "$NVMPI_CUDA_OXIDE"
git fetch --depth 1 origin "$rev"
git checkout --detach "$rev"

# cuda-oxide floors at Volta; on Maxwell laptops the backend must be patched.
# Patch is idempotent; the flag file marks "already patched + backend rebuilt".
flag="$NVMPI_CUDA_OXIDE/.maxwell-patched"
if [ -n "${CUDA_OXIDE_TARGET:-}" ] && [ ! -f "$flag" ]; then
	echo "[prep] patching cuda-oxide backend for ${CUDA_OXIDE_TARGET}"
	python3 - <<'PY'
import re
p = "crates/cuda-oxide-codegen/src/target.rs"
s = open(p).read()
s = s.replace("DetectedFeatures::Basic => major >= 7", "DetectedFeatures::Basic => major >= 5")
s = s.replace("70 | 71 | 72 |", "50 | 52 | 53 | 70 | 71 | 72 |")
open(p, "w").write(s)
PY
	touch "$flag"
fi

run cargo oxide setup

# Build the nvmpi cargo subcommand so tests can run `cargo nvmpi run`.
cd "$NVMPI_PROJECT/trame/cargo-nvmpi"
run cargo build --release