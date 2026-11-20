#!/bin/sh
# Run a backend's tests under Miri with Tree Borrows in the miri dev shell, from the repository root:
#   nix develop .#miri --command sh check/proofs/miri.sh BACKEND
# BACKEND is a directory of src/backends/ whose descriptor has `cargoTests = true`: the tests are cargo's own.
# The suites are the library's unit tests, tests/conformance.rs, tests/parallel.rs and tests/alloc.rs.
# Miri's isolation stays on, so a test that reads the clock or the environment fails rather than passes.
# `math::` is skipped: it has no unsafe code, and Miri perturbs std's exp and ln by a few ulp on purpose, which is
# the oracle those tests compare against, so they fail there and say nothing about undefined behaviour.
# The host's own wait for a deadline is virtual time. Exit status is non-zero when Miri reports undefined
# behaviour or a test fails; the names of the suites that failed are printed.
#
# Each job has its own target directory. Measured: with one shared directory, the concurrent cargo processes waited on
# its lock ("Blocking waiting for file lock on build directory") and the suites ran one after another. The Miri sysroot
# is shared (it lives under ~/.cache/miri); `cargo miri setup` builds it once before the jobs start. The slow tests
# (sched::choose_is_the_best..., the three *_traces_follow_the_model, edits_follow_the_model) each get a process.
set -u
export RUSTFLAGS="--cfg trame_checked"
export MIRIFLAGS="-Zmiri-tree-borrows ${MIRIFLAGS:-}"
base="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/trame-miri-target}"
backend=${1:?usage: miri.sh BACKEND (a directory of src/backends/ whose tests cargo runs)}
[ -f "src/backends/$backend/backend.nix" ] || { echo "miri.sh: no backend $backend in src/backends/" >&2; exit 1; }
F="--no-default-features --features $backend"
logs="${TMPDIR:-/tmp}/trame-miri-logs.$$"
mkdir -p "$logs" || exit 1

cargo miri setup || { echo "miri.sh: cargo miri setup failed" >&2; exit 1; }

# One job per process: name, then the cargo arguments. The slow tests (choose_is_the_best..., edits_follow..., and the
# three *_traces_follow_the_model) each get a process of their own, so the wall time is the slowest of them, not their sum.
run() {
    name=$1
    shift
    "$@" > "$logs/$name.log" 2>&1
    echo $? > "$logs/$name.status"
}
job() {
    name=$1
    shift
    run "$name" env CARGO_TARGET_DIR="$base-$name" cargo miri test $F "$@" &
}
job lib-sched --lib -- --skip math:: sched::
job lib-backends --lib -- --skip math:: backends::
job lib-rest --lib -- --skip math:: --skip sched:: --skip backends::
job conf-edits --test conformance -- --exact edits_follow_the_model
job conf-hot --test conformance -- --exact hot_traces_follow_the_model
job conf-readers --test conformance -- --exact readers_traces_follow_the_model
job conf-chain --test conformance -- --exact chain_traces_follow_the_model
job conf-rest --test conformance -- --skip edits_follow_the_model --skip hot_traces_follow_the_model --skip readers_traces_follow_the_model --skip chain_traces_follow_the_model
job parallel --test parallel
job alloc --test alloc
wait

failed=""
for name in lib-sched lib-backends lib-rest conf-edits conf-hot conf-readers conf-chain conf-rest parallel alloc; do
    cat "$logs/$name.log"
    status=$(cat "$logs/$name.status")
    if [ "$status" -ne 0 ]; then
        failed="$failed $name"
        echo "miri.sh: $name exited $status" >&2
    fi
done
# Totals per suite, so the counts can be checked against the suites' tests.
for suite in lib conf parallel alloc; do
    cat "$logs"/$suite*.log | awk -v s="$suite" '/^test result:/ { p += $4; f += $6; i += $8 }
        END { printf "miri.sh: %s: %d passed, %d failed, %d ignored\n", s, p, f, i }'
done
rm -rf "$logs"
if [ -n "$failed" ]; then
    echo "miri.sh: FAILED:$failed" >&2
    exit 1
fi
echo "miri.sh: all suites passed"
