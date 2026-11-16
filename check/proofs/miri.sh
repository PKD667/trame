#!/bin/sh
# Run the seq backend's tests under Miri with Tree Borrows in the miri dev shell, from the repository root:
#   nix develop .#miri --command sh check/proofs/miri.sh
# The suites are the library's unit tests, tests/conformance.rs, tests/parallel.rs and tests/alloc.rs.
# Miri's isolation stays on, so a test that reads the clock or the environment fails rather than passes.
# The host's own wait for a deadline is virtual time. Exit status is non-zero when Miri reports undefined
# behaviour or a test fails.
set -eu
export RUSTFLAGS="--cfg trame_checked"
export MIRIFLAGS="-Zmiri-tree-borrows ${MIRIFLAGS:-}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/trame-miri-target}"
F="--no-default-features --features seq"
cargo miri test $F --lib
for t in conformance parallel alloc; do cargo miri test $F --test "$t"; done
