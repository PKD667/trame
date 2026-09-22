#!/bin/sh
# Run a test on a remote host. Usage:
#   trame/nv/build/test.sh <test>                (remote only)
#   trame/nv/build/test.sh remote <test>
set -eu

here=$(CDPATH= cd "$(dirname "$0")" && pwd -P)

count=$#
i=0
backends=""
test_name=""
for a in "$@"; do
	i=$((i + 1))
	if [ "$i" -eq "$count" ]; then
		test_name=$a
	else
		backends="$backends $a"
	fi
done
[ -n "$test_name" ] || test_name=pingpong
[ -n "$backends" ] || backends=" remote"

for b in $backends; do
	[ -x "$here/$b.sh" ] || { echo "[test] unknown backend: $b"; exit 1; }
	echo
	echo "### backend: $b"
	"$here/$b.sh" "$test_name"
done