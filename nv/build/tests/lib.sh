#!/bin/sh
# Shared helpers for build/tests scripts. Source, don't exec.
#
# Env (all optional; the defaults fit a bare host with a normal home):
#   NVMPI_BASE       : sandbox on the target host holding nerve
#   NVMPI_PROJECT    : nerve workspace (defaults to $NVMPI_BASE/nerve)
#   NVMPI_CUDA_OXIDE : cuda-oxide checkout (defaults to $NVMPI_BASE/cuda-oxide)
#   NVMPI_RUSTUP_HOME, NVMPI_CARGO_HOME, NVMPI_TARGET_DIR, NVMPI_PREFIX:
#                     relocation for quota-cramped homes, e.g. /local on g5k
#   CUDA_OXIDE_TARGET: set by lib.sh for pre-Volta GPUs
#
# CUDA_HOME / LD_LIBRARY_PATH / LIBCLANG_PATH are detected here so every
# phase shares the same environment; prep.sh fetches whatever is missing.

: "${NVMPI_BASE:=$HOME/nvmpi-test}"
: "${NVMPI_PROJECT:=$NVMPI_BASE/nerve}"
: "${NVMPI_CUDA_OXIDE:=$NVMPI_BASE/cuda-oxide}"
: "${NVMPI_RUSTUP_HOME:=$HOME/.rustup}"
: "${NVMPI_CARGO_HOME:=$HOME/.cargo}"
: "${NVMPI_PREFIX:=$HOME/nvmpi-test/deps}"
export RUSTUP_HOME="$NVMPI_RUSTUP_HOME" CARGO_HOME="$NVMPI_CARGO_HOME"

export NVMPI_PREFIX

# CUDA toolkit headers for host cuda-bindings.
#
# The last entry is where Debian's nvidia-cuda-dev unpacks: `usr/include/cuda.h` under a prefix,
# with no `lib/cuda` directory. The Ubuntu-style path before it is where the same package lands on
# the distributions that ship under `/usr/lib/cuda`. Both are probed because the package, not the
# distribution, decides, and a header that is present but not found is the failure this loop
# exists to prevent.
#
# The first entry is the toolkit whose `nvcc` is on PATH: where a toolkit is an Lmod module (g5k),
# loading it is the operator's statement of which toolkit to use, and its root is nowhere else.
nvcc=$(command -v nvcc || true)
for d in ${nvcc:+"${nvcc%/bin/nvcc}"} /usr/local/cuda /usr/lib/cuda "$NVMPI_PREFIX/usr/lib/cuda" "$NVMPI_PREFIX/usr"; do
	if [ -f "$d/include/cuda.h" ]; then
		export CUDA_HOME="$d" CUDA_TOOLKIT_PATH="$d"
		break
	fi
done

# NVVM runtime for PTX finalization, and libclang for bindgen.
# NVMPI_NVVM_DIR is the shared "found" marker prep.sh reads.
# A module toolkit keeps libnvvm under its own `lib`, so the toolkit found above is asked first.
for d in ${CUDA_HOME:+"$CUDA_HOME/lib"} /usr/lib/x86_64-linux-gnu /usr/local/cuda/lib64 "$NVMPI_PREFIX/usr/lib/x86_64-linux-gnu"; do
	if [ -n "$(ls "$d"/libnvvm.so* 2>/dev/null | head -1)" ]; then
		export NVMPI_NVVM_DIR="$d"
		case ":${LD_LIBRARY_PATH:-}:" in *":$d:"*) ;; *)
			export LD_LIBRARY_PATH="$d${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" ;;
		esac
		break
	fi
done
# The check is `-e` and not a glob match, because `libclang-19-dev` ships a `libclang-19.so`
# symlink whose target is in the *runtime* package. A directory holding only the dangling link
# satisfies a glob and fails bindgen, which is exactly the failure this detection exists to avoid.
if [ -z "${LIBCLANG_PATH:-}" ]; then
	# The fetched copy first: it is the one this run pinned, and a system copy that happens to
	# exist should not decide which libclang the build used. The numeric pattern is deliberate —
	# `libclang-cpp.so` matches a looser glob and is a different library.
	for d in "$NVMPI_PREFIX/usr/lib/x86_64-linux-gnu" /usr/lib/llvm-*/lib /usr/lib/x86_64-linux-gnu; do
		for f in "$d"/libclang.so "$d"/libclang.so.* "$d"/libclang-[0-9]*.so "$d"/libclang-[0-9]*.so.*; do
			[ -e "$f" ] && { export LIBCLANG_PATH="$d"; break 2; }
		done
	done
fi

# libclang's builtin headers. Extracting a libclang from a package does not extract the headers
# clang provides for itself, so a translation unit that includes <stdlib.h> fails on <stddef.h>
# unless the resource directory is named. The prefix first, then wherever the system keeps one.
if [ -z "${BINDGEN_EXTRA_CLANG_ARGS:-}" ]; then
	for d in "$NVMPI_PREFIX"/usr/lib/llvm-*/lib/clang/*/include /usr/lib/llvm-*/lib/clang/*/include /usr/lib/clang/*/include; do
		[ -e "$d/stddef.h" ] && { export BINDGEN_EXTRA_CLANG_ARGS="-isystem $d"; break; }
	done
fi

# nix hosts run every cargo invocation inside the cuda-oxide dev shell;
# hosts without nix run bare.
run() {
	if [ -n "${IN_NIX:-}" ] || ! command -v nix >/dev/null 2>&1; then
		"$@"
	else
		nix develop "$NVMPI_CUDA_OXIDE" --command "$@"
	fi
}

# The cargo-nv subcommand is built by prep.sh into the target dir.
if [ -n "${NVMPI_TARGET_DIR:-}" ]; then
	export CARGO_TARGET_DIR="$NVMPI_TARGET_DIR"
	bin_dir="$NVMPI_TARGET_DIR/release"
else
	bin_dir="$NVMPI_PROJECT/trame/nv/cargo/target/release"
fi
export PATH="$CARGO_HOME/bin:$bin_dir:$PATH"

# Pre-Volta GPUs (< 70) need an explicit target and a backend patch.
cc=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1)
case "${cc%.*}" in
"") ;;
[0-6]) export CUDA_OXIDE_TARGET=sm_$(echo "$cc" | tr -d .) ;;
esac