#!/usr/bin/env bash
# P1: the public surface is the same on `none`, nv and mpi.
#
# A caller compiles against one backend and runs on another, so an item, or an auto trait, that
# exists on one build and not another is a program that builds on the first and breaks on the
# second. rustdoc states both: `all.html` lists every public item, and each struct page's
# synthetic implementations are its auto traits (`Send`, `Sync`, ...) as the compiler derived
# them. The pages that state backend-specific values (`MAX_FRAME`, `LOSSY`, `ID`) are not read,
# so what differs by design never enters the comparison.
#
# Each build documents into its own CARGO_TARGET_DIR with RUSTDOCFLAGS empty, so neither a
# previous build's pages nor the environment's flags can stand in for this one's. mpi documents
# under `nix develop`, where its MPI is. Exit 0 when all three agree, 1 with the diff otherwise.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
out=${1:-$(mktemp -d)}
mkdir -p "$out"

document() {
	local feature=$1 dir=target-doc-$1
	local args=(-q -p trame --no-deps)
	[ "$feature" = none ] || args+=(--features "$feature")
	rm -rf "$dir/doc/trame"
	if [ "$feature" = mpi ]; then
		RUSTDOCFLAGS= CARGO_TARGET_DIR=$dir nix develop -c cargo doc "${args[@]}" >/dev/null 2>"$out/doc-$feature.log"
	else
		RUSTDOCFLAGS= CARGO_TARGET_DIR=$dir cargo doc "${args[@]}" 2>"$out/doc-$feature.log"
	fi
	local doc=$dir/doc/trame
	# Only the pages `all.html` names: the others are redirects rustdoc leaves at a re-exported
	# item's defining path, which is the backend's module and so differs by construction.
	grep -o '<li><a href="[^"]*">' "$doc/all.html" | sed 's/<li><a href="\([^"]*\)">/\1/' | sort >"$out/items-$feature.txt"
	{
		echo "## items"
		cat "$out/items-$feature.txt"
		grep '\(^\|/\)struct\.' "$out/items-$feature.txt" | while read -r item; do
			local page=$doc/$item
			echo "## $item"
			sed -n 's/.*id="synthetic-implementations-list"\(.*\)id="blanket-implementations".*/\1/p' "$page" |
				sed 's/<h3 class="code-header">/\n&/g; s/<\/h3>/&\n/g' | sed -n 's/^<h3 class="code-header">//p' |
				sed 's/<[^>]*>//g; s/&lt;/</g; s/&gt;/>/g; s/&amp;/\&/g' | sort
		done
	} >"$out/surface-$feature.txt"
}

for feature in none nv mpi; do
	document "$feature"
done

status=0
for feature in nv mpi; do
	if ! diff -u "$out/surface-none.txt" "$out/surface-$feature.txt" >"$out/surface-$feature.diff"; then
		echo "P1: none and $feature differ:"
		cat "$out/surface-$feature.diff"
		status=1
	fi
done
[ "$status" -ne 0 ] || echo "P1: $(grep -vc '^##' "$out/surface-none.txt") lines of items and auto traits agree on none, nv, mpi"
exit "$status"
