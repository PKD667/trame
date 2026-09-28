#!/usr/bin/env bash
# The conformance suite: every claim of `trame/conformance/claims.rs` on every backend this target
# can run, then a table of claim × backend.
#
#     bash trame/scripts/conform.sh local
#
# A cell passes when the claim reported, every participant's verdict is pass, and no participant
# started it without finishing. A claim that never reported is a FAIL, and so is one that was
# running when its launch hit `timeout`: the start line each participant prints before a claim is
# what names it. `MPIRUN` replaces the launcher prefix (default `mpirun --oversubscribe`).
#
# Besides the body: X1 is `cargo test -p trame` per backend, X2 the declaration fixtures, P1 the
# public surface (`surface.sh`). Exit 0 only when every cell passes.
set -uo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
here=$root/trame/scripts
cd "$root"

# Seconds a launch may run. S1 waits without a spin bound; the launcher's `timeout "$LAUNCH"` is
# its only liveness detector. A worker that fails before its DONE leaves the leader waiting until
# that timeout, which reports FAIL.
LAUNCH=${LAUNCH:-150}
WORKERS=4
LEADERS=2

# The MPI half, re-entered inside `nix develop`, where MPI is: build and run the example per
# backend, then the backend's unit tests.
if [ "${1:-}" = mpi-stage ]; then
	out=$2
	for backend in mpi rma rma-lossy; do
		dir=target-${backend#rma-}
		bin=$dir/debug/examples/conformance
		if ! CARGO_TARGET_DIR=$dir cargo build -q -p trame --features "$backend" --example conformance 2>"$out/$backend.build.log"; then
			echo "{\"claim\":\"X1\",\"backend\":\"$backend\",\"participant\":\"cargo\",\"verdict\":\"fail\",\"detail\":\"the conformance example does not build\"}" >"$out/$backend.build.jsonl"
			continue
		fi
		# shellcheck disable=SC2086
		TRAME_WORKERS=$WORKERS TRAME_LEADERS=$LEADERS timeout "$LAUNCH" ${MPIRUN:-mpirun --oversubscribe} \
			-n "$WORKERS" "$bin" worker : -n 1 "$bin" leader "$WORKERS" : -n 1 "$bin" leader $((WORKERS + 1)) \
			>"$out/$backend.main.jsonl" 2>"$out/$backend.main.log"
		echo $? >"$out/$backend.main.status"
		# shellcheck disable=SC2086
		TRAME_WORKERS=$WORKERS TRAME_LEADERS=0 timeout "$LAUNCH" ${MPIRUN:-mpirun --oversubscribe} \
			-n "$WORKERS" "$bin" pressure \
			>"$out/$backend.pressure.jsonl" 2>"$out/$backend.pressure.log"
		echo $? >"$out/$backend.pressure.status"
		CARGO_TARGET_DIR=$dir timeout "$LAUNCH" cargo test -q -p trame --features "$backend" >"$out/$backend.x1.log" 2>&1
		echo $? >"$out/$backend.x1.status"
	done
	exit 0
fi

[ "${1:-}" = local ] || { echo "usage: conform.sh local" >&2; exit 2; }
started=$(date +%s)
out=$(mktemp -d)
echo "conform: output in $out"

# The in-process backends' own tests. On nv's host model, also the body as an ignored test, the
# claims and M5 as separate invocations; `none` carries no frames, so it answers no claim.
for backend in none nv; do
	features=()
	[ "$backend" = none ] || features=(--features nv)
	cargo test -q -p trame "${features[@]}" --lib --no-run 2>"$out/$backend.build.log"
	if [ "$backend" = nv ]; then
		for launch in claims pressure; do
			timeout "$LAUNCH" cargo test -q -p trame "${features[@]}" --lib "nv::tests::conformance::$launch" \
				-- --ignored --exact --nocapture >"$out/$backend.$launch.raw" 2>"$out/$backend.$launch.log"
			echo $? >"$out/$backend.$launch.status"
			grep '^{"claim"' "$out/$backend.$launch.raw" >"$out/$backend.$launch.jsonl"
		done
	fi
	timeout "$LAUNCH" cargo test -q -p trame "${features[@]}" >"$out/$backend.x1.log" 2>&1
	echo $? >"$out/$backend.x1.status"
done

nix develop -c bash "$here/conform.sh" mpi-stage "$out" >"$out/nix.log" 2>&1

timeout "$LAUNCH" bash "$here/declare/check.sh" --fixtures >"$out/x2.log" 2>&1
echo $? >"$out/x2.status"
timeout "$LAUNCH" bash "$here/surface.sh" "$out/surface" >"$out/p1.log" 2>&1
echo $? >"$out/p1.status"

# One verdict line per cell that is not a claim of the body, from its exit status.
status_line() {
	local claim=$1 backend=$2 file=$3 log=$4 code verdict=pass
	code=$(cat "$file" 2>/dev/null || echo missing)
	[ "$code" = 0 ] || verdict=fail
	jq -cn --arg c "$claim" --arg b "$backend" --arg v "$verdict" --arg d "exit $code; $(tail -c 300 "$log" 2>/dev/null | tr '\n' ' ')" \
		'{claim:$c,backend:$b,participant:"script",verdict:$v,detail:$d}'
}

backends=(none nv mpi rma rma-lossy)
claims=$(sed -n '/^pub const CLAIMS/,/^];/p' "$root/trame/conformance/claims.rs" | grep -o '"[A-Z0-9]*"' | tr -d '"')
{
	for backend in "${backends[@]}"; do
		cat "$out/$backend".*.jsonl 2>/dev/null
		# A participant that started a claim and has no verdict for it: its launch ended inside it.
		for launch in claims pressure main; do
			[ -e "$out/$backend.$launch.status" ] || continue
			code=$(cat "$out/$backend.$launch.status")
			why="launch exited $code"
			[ "$code" = 124 ] && why="timed out after ${LAUNCH}s"
			jq -c --arg why "$why" --arg b "$backend" -s '
				(map(select(.verdict)) | map({claim, participant})) as $done
				| map(select(.event == "start") | {claim, participant})
				| unique - $done | .[]
				| {claim, backend: $b, participant, verdict: "fail", detail: "no verdict: \($why) while running it"}' \
				"$out/$backend.$launch.jsonl"
			# A launch that failed with every claim reported is a failure too; its log says of what.
			if [ "$code" != 0 ] && [ "$code" != 124 ] &&
				! jq -e -s 'any(.verdict == "fail")' "$out/$backend.$launch.jsonl" >/dev/null; then
				jq -cn --arg b "$backend" --arg d "launch $launch exited $code with no claim failing" \
					'{claim:"X1",backend:$b,participant:"script",verdict:"fail",detail:$d}'
			fi
		done
		status_line X1 "$backend" "$out/$backend.x1.status" "$out/$backend.x1.log"
	done
	status_line X2 none "$out/x2.status" "$out/x2.log"
	for backend in none nv mpi; do
		status_line P1 "$backend" "$out/p1.status" "$out/p1.log"
	done
} >"$out/all.jsonl"

# The table. A cell with no verdict line is a FAIL, not a blank: the inventory says it should report.
failed=0
printf '%-4s' claim
printf ' %-13s' "${backends[@]}"
echo
for claim in $claims X1 X2 P1; do
	printf '%-4s' "$claim"
	for backend in "${backends[@]}"; do
		cell=$(jq -r -s --arg c "$claim" --arg b "$backend" '
			map(select(.claim == $c and .backend == $b and .verdict))
			| if length == 0 then "none" elif all(.verdict == "pass") then "pass" else "FAIL" end' "$out/all.jsonl")
		# X2 is one run for all backends, and P1 compares the three it names.
		case "$claim:$backend" in
			X2:none | P1:none | P1:nv | P1:mpi) ;;
			X2:* | P1:*) cell=n/a ;;
			X1:*) ;;
			S1:nv)
				if jq -e -s '
					[.[] | select(.claim == "S1" and .backend == "nv" and .verdict)] as $s
					| ($s | length) == 5
					  and ($s | map(.participant) | sort) == ["leader 4", "worker 0", "worker 1", "worker 2", "worker 3"]
					  and all($s[];
					      .verdict == "pass" and
					      (if .participant == "leader 4"
					       then (.detail | startswith("UNIMPLEMENTED: publish refused with BackendFault::Unimplemented"))
					       else (.detail | startswith("UNIMPLEMENTED: attach refused with BackendFault::Unimplemented")) end))
				' "$out/all.jsonl" >/dev/null; then
					cell=UNIMPLEMENTED
				else
					cell=FAIL
				fi
				;;
			# The backend that does nothing carries no claim's frames.
			*:none) cell=n/a ;;
		esac
		[ "$cell" = none ] && cell="FAIL(missing)"
		case $cell in FAIL*) failed=1 ;; esac
		printf ' %-13s' "$cell"
	done
	echo
done

echo
jq -r 'select(.verdict == "fail") | "\(.backend) \(.claim) \(.participant): \(.detail)"' "$out/all.jsonl" | cut -c1-400
echo "conform: $(( $(date +%s) - started ))s; logs in $out"
exit "$failed"
