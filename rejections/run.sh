#!/usr/bin/env bash
# The rejection suite (rejections/README.md): every case in cases/<rule>/<name>.rs is checked against
# the REAL trame crate, and its outcome is compared with the case's expectation.
#
#   rejections/run.sh [substring]     run the cases whose path contains the substring
#
# Works from any shell. It re-enters the shell of driver/ (the pinned nightly, driver/rust-toolchain.toml)
# with nix, because the driver refuses to run under another rustc. Set TRAME_REJECTIONS_INNER=1 to
# skip the re-entry, and TRAME_DRIVER_BIN=<dir with cargo-trame and trame-driver> to skip the nix build
# (driver/tests/rejections.rs does both).
#
# Exit status: 0 when every case matches, or is a known gap (a `//! known:` line: the driver or a macro
# disagrees with the spec, and the table says how). 1 on any other mismatch, and on a known gap that now
# matches (remove its `known` line).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
filter="${1:-}"

if [ -z "${TRAME_REJECTIONS_INNER:-}" ]; then
  exec nix develop "path:$root#driver" --command env TRAME_REJECTIONS_INNER=1 "$here/run.sh" "$@"
fi

# RUSTFLAGS would reach every case. A plain-cargo case must see no --cfg, and the driver adds
# `--cfg trame_checked` itself to the crates it compiles.
unset RUSTFLAGS

if [ -z "${TRAME_DRIVER_BIN:-}" ]; then
  TRAME_DRIVER_BIN="$(nix build "path:$root#trame-driver" --no-link --print-out-paths)/bin"
fi
export PATH="$TRAME_DRIVER_BIN:$PATH"
command -v cargo-trame >/dev/null && command -v trame-driver >/dev/null || {
  echo "run.sh: cargo-trame and trame-driver must be in $TRAME_DRIVER_BIN" >&2
  exit 1
}

cd "$here"

# The [[example]] list of Cargo.toml, from cases/. Cargo cannot glob examples, so the block between the
# markers is rewritten on every run: a new case file is run without an edit to the manifest.
begin=$(grep -n '^# BEGIN generated examples' Cargo.toml | cut -d: -f1)
end=$(grep -n '^# END generated examples' Cargo.toml | cut -d: -f1)
{
  head -n "$begin" Cargo.toml
  for f in $(find cases -mindepth 2 -maxdepth 2 -name '*.rs' | sort); do
    dir="$(basename "$(dirname "$f")")"
    base="$(basename "$f" .rs)"
    printf '\n[[example]]\nname = "%s__%s"\npath = "%s"\n' "${dir//-/_}" "$base" "$f"
  done
  tail -n +"$end" Cargo.toml
} > Cargo.toml.new
mv Cargo.toml.new Cargo.toml

# A line `//! key: value` among the first five lines of a case.
# (grep, not sed | head: under pipefail, head closing the pipe would end the script.)
header() { local l; l=$(head -n 5 "$1" | grep -m1 "^//! $2:" || true); l=${l#//! $2:}; l=${l# }; printf '%s\n' "$l"; }

logs="$here/target/rejections-logs"
mkdir -p "$logs"
rm -f "$logs"/*.log

pass=0; gap=0; fail=0
printf '%-4s %-44s %-36s %s\n' RESULT CASE EXPECT GOT
for f in $(find cases -mindepth 2 -maxdepth 2 -name '*.rs' | sort); do
  case "$f" in *"$filter"*) ;; *) continue ;; esac
  name="${f#cases/}"; name="${name%.rs}"
  ex="$(dirname "$name" | tr - _)__$(basename "$name")"
  expect="$(header "$f" expect)"
  runner="$(header "$f" runner)"
  known="$(header "$f" known)"

  # The cached artifacts of this case are removed first: cargo does not see the driver binary in its
  # fingerprints, so an "accepted" result could otherwise be replayed from a build the driver never judged.
  cargo clean -q -p rejections >/dev/null 2>&1 || true
  cargo clean -q -p rejections --target-dir "$here/target/trame" >/dev/null 2>&1 || true
  if [ "$runner" = "plain cargo" ]; then
    cmd=(cargo check -q --color never --example "$ex")
  else
    cmd=(cargo trame check --color never --example "$ex")
  fi
  set +e
  out="$("${cmd[@]}" 2>&1)"
  st=$?
  set -e
  printf '%s\n' "$out" > "$logs/${name//\//__}.log"

  # The driver's refusals: `error: [rule] level ...` (driver/src/check/mod.rs). `[root]` has no level.
  obs="$({ grep -E '^error: \[' <<<"$out" || true; } | sed -E 's/^error: \[([^]]+)\] (item|step|worker) .*/\1 \2/; s/^error: \[([^]]+)\].*/\1 root/' | sort -u | paste -sd, -)"
  # rustc's own errors and macro errors: any `error` that is not a driver refusal or cargo's summary.
  other="$(grep -E '^error' <<<"$out" | grep -vE '^error: \[|^error: could not compile|^error: aborting due to' || true)"
  first="$(grep -m1 -E '^error' <<<"$out" || true)"

  case "$expect" in
    accepted)
      want="accepted"
      if [ "$st" -eq 0 ]; then ok=1; else ok=0; fi
      got="accepted"; [ "$st" -eq 0 ] || got="refused: ${obs:-$first}"
      ;;
    "refused ["*)
      # `refused [rule] level, [rule] level, ...`: the exact set of driver refusals (a case refused for a
      # reason it does not name fails). The rule and level pairs are sorted as `obs` is.
      want="${expect}"
      want_set="$(tr ',' '\n' <<<"${expect#refused }" | sed -E 's/^ *\[([^]]+)\] (item|step|worker) *$/\1 \2/' | sort -u | paste -sd, -)"
      if [ "$st" -ne 0 ] && [ "$obs" = "$want_set" ] && [ -z "$other" ]; then ok=1; else ok=0; fi
      got="accepted"; [ "$st" -eq 0 ] || got="${obs:-error: ${first}}"
      [ -n "$other" ] && got="$got; other: $(head -n1 <<<"$other")"
      ;;
    "error "*)
      want="error ${expect#error }"
      if [ "$st" -ne 0 ] && grep -qF -- "${expect#error }" <<<"$out" && [ -z "$obs" ]; then ok=1; else ok=0; fi
      got="accepted"; [ "$st" -eq 0 ] || got="${first}"
      ;;
    *)
      echo "run.sh: $f: no valid expect line (accepted | refused [rule] level | error <substring>)" >&2
      exit 1
      ;;
  esac

  if [ "$ok" -eq 1 ] && [ -z "$known" ]; then
    res=PASS; pass=$((pass + 1))
  elif [ "$ok" -eq 1 ]; then
    res=XPASS; fail=$((fail + 1))
  elif [ -n "$known" ]; then
    res=GAP; gap=$((gap + 1))
  else
    res=FAIL; fail=$((fail + 1))
  fi
  printf '%-4s %-44s %-36s %s\n' "$res" "$name" "$want" "$got"
  if [ "$res" = GAP ] || [ "$res" = FAIL ] || [ "$res" = XPASS ]; then
    [ -n "$known" ] && printf '     known: %s\n' "$known"
    printf '     driver/macro said: %s\n' "${first:-(no error line)}"
  fi
done

echo
echo "$pass pass, $gap known gap, $fail fail (logs: ${logs#$root/}/)"
[ "$fail" -eq 0 ]
