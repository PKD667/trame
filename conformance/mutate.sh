#!/usr/bin/env bash
# Mutation runner (conformance/README.md). For each patch in conformance/mutants/<backend>/, restore a
# copy of the working tree, apply the patch, build, and run programs/run.sh for that backend. A mutant is
# killed when a program fails (stdout differs, non-zero exit or timeout); a mutant that does not build is
# reported as BUILD and counts as a failure of the catalogue, not as a kill.
#   nix develop .#mpi --command conformance/mutate.sh [seq|mpi|both] [mutant-id-regex]
# The two backends run concurrently, each in its own copy, /tmp/trame-mutate/<backend> (or $TRAME_MUTATE_DIR/<backend>),
# which is kept between runs with its target directory, so Cargo rebuilds only what a patch changed. A lock
# on the copy refuses a second run of the same backend while one runs. Exits non-zero when a verdict
# contradicts its header: a `killed` mutant survives, or an `equivalent` one is killed, or a build fails.
set -uo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
which_backend="${1:-both}"
filter="${2:-.}"
case "$which_backend" in seq|mpi|both) ;; *) echo "usage: $0 [seq|mpi|both] [regex]" >&2; exit 2 ;; esac
backends=(seq mpi)
[[ "$which_backend" == both ]] || backends=("$which_backend")
out="$(mktemp -d)"
scratch="${TRAME_MUTATE_DIR:-/tmp/trame-mutate}"

# Restore the copy from the working tree, without its target directories or the mpi-rma submodule's .git (the crate itself is a path dependency).
sync_copy() {
  local work="$1"
  mkdir -p "$work"
  rsync -rlD --delete --no-times --checksum \
    --exclude=/target/ --exclude=/.git --exclude=/mpi-rma/.git --exclude=/mpi-rma/target/ \
    "$root/" "$work/"
}

# One backend: every patch in order. Writes one line per mutant to $out/<backend>.tsv.
run_backend() {
  local b="$1" work="$scratch/$1" tsv="$out/$1.tsv"
  # One run per copy: a second run of this backend would rebuild under the first. Refuse it.
  mkdir -p "$scratch"
  exec {lock}>"$scratch/$b.lock"
  flock -n "$lock" || { echo "mutate.sh: another run holds $work ($scratch/$b.lock)" >&2; exit 2; }
  : > "$tsv"
  for patch in "$root/conformance/mutants/$b"/*.patch; do
    local id header expect rule verdict prog detail
    id="$(basename "$patch" .patch)"
    [[ "$id" =~ $filter ]] || continue
    header="$(grep -m1 '^# violates:' "$patch" | sed 's/^# violates: *//')"
    expect="$(grep -m1 '^# expect:' "$patch" | sed 's/^# expect: *//')"
    sync_copy "$work"
    if ! (cd "$work" && git apply "$patch" 2>"$out/$b.$id.apply"); then
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$b" "$expect" APPLY "-" "$(head -c 200 "$out/$b.$id.apply")" >> "$tsv"
      continue
    fi
    # Build first: a mutant that does not compile must not look like a program failure.
    # Same flags and target directory as programs/run.sh, so run.sh's build is then a no-op.
    if [[ "$b" == seq ]]; then
      (cd "$work" && RUSTFLAGS='--cfg trame_checked' cargo build -q --examples >"$out/$b.$id.build" 2>&1)
    else
      (cd "$work" && CARGO_TARGET_DIR="$work/target/mpi" RUSTFLAGS='--cfg trame_checked' cargo build -q --no-default-features --features mpi --examples >"$out/$b.$id.build" 2>&1)
    fi
    if [[ $? -ne 0 ]]; then
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$b" "$expect" BUILD "-" "$(grep -m1 error "$out/$b.$id.build" | head -c 200)" >> "$tsv"
      continue
    fi
    local log="$out/$b.$id.run"
    (cd "$work" && programs/run.sh "$b") >"$log" 2>&1
    if [[ $? -eq 0 ]]; then
      verdict=survived; prog="-"; detail="all programs passed"
    else
      verdict=killed
      prog="$(grep -m1 '^FAIL ' "$log" | sed -E "s/^FAIL $b ([a-z_]+).*/\1/")"
      [[ -n "$prog" ]] || prog="?"
      detail="$(grep -m1 '^FAIL ' "$log" | head -c 200)"
    fi
    pkill -f "$work/target/" 2>/dev/null || true
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$b" "$expect" "$verdict" "$prog" "$detail" >> "$tsv"
    echo "[$b] $id: $verdict ${prog}" >&2
  done
}

pids=()
for b in "${backends[@]}"; do run_backend "$b" & pids+=($!); done
# A backend that could not run (its copy is locked) fails the whole run.
for pid in "${pids[@]}"; do wait "$pid" || exit 2; done

# The table and the exit status: a verdict that contradicts its header is an error.
bad=0
printf '%-8s %-5s %-28s %-9s %-18s %s\n' mutant backend expect verdict program detail
for b in "${backends[@]}"; do
  while IFS=$'\t' read -r id be expect verdict prog detail; do
    kind="${expect%%:*}"
    mark=""
    case "$kind:$verdict" in
      killed:killed|equivalent:survived) mark="" ;;
      *) mark="  <-- CONTRADICTS EXPECT"; bad=1 ;;
    esac
    printf '%-8s %-5s %-28s %-9s %-18s %s%s\n' "$id" "$be" "$kind" "$verdict" "$prog" "${detail:0:90}" "$mark"
  done < "$out/$b.tsv"
done
echo "results in $out"
exit $bad
