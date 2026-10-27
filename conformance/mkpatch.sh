#!/usr/bin/env bash
# Writes one mutant patch, conformance/mutants/<backend>/<id>.patch, as a unified diff of one file.
#   conformance/mkpatch.sh <backend> <id> <rule> <expect> <path> <perl-substitution>
# <expect> is `killed` or `equivalent: <argument>`. The substitution runs as `perl -0pi -e` on a copy of
# <path>. It must change the file (a no-op is an error: the mutant would be vacuous), and the patch must
# apply with `git apply --check` to the working tree (the header lines are skipped by git apply).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
backend="$1" id="$2" rule="$3" expect="$4" path="$5" subst="$6"
dir="conformance/mutants/$backend"
mkdir -p "$dir"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/a/$(dirname "$path")" "$tmp/b/$(dirname "$path")"
cp "$path" "$tmp/a/$path"
cp "$path" "$tmp/b/$path"
perl -0pi -e "$subst" "$tmp/b/$path"
if cmp -s "$tmp/a/$path" "$tmp/b/$path"; then
  echo "mkpatch: $id: the substitution did not change $path" >&2
  exit 1
fi
out="$dir/$id.patch"
{
  echo "# violates: $rule"
  echo "# expect: $expect"
  (cd "$tmp" && diff -u --label "a/$path" --label "b/$path" "a/$path" "b/$path") || true
} > "$out"
git apply --check "$out"
echo "wrote $out"
