#!/bin/sh
# Runs TLC on every model of proofs/mpi, and writes OUTDIR/outcome.json (the nixsci verdict format),
# OUTDIR/results.txt (one line per model), OUTDIR/logs/ (TLC's output) and OUTDIR/counterexamples/.
# Usage: TLC=<tlc binary> sh run.sh OUTDIR. A model passes when TLC's output contains its expected line:
# "No error has been found" for a property that holds, "is violated" for a counterexample that must exist.
set -u
here=$(cd "$(dirname "$0")" && pwd)
out=$1
tlc=${TLC:-tlc}
work=$(mktemp -d)
cp -r "$here"/. "$work"/ && chmod -R u+w "$work"
mkdir -p "$out/logs" "$out/counterexamples"
: > "$out/results.txt"
holds=true
items=""
# name : root module (its .cfg holds the constants) : the line TLC must print
while IFS=: read -r name mod expect; do
  rc=0
  (cd "$work" && "$tlc" -workers 4 -deadlock "$mod.tla" > "$out/logs/$name.log" 2>&1) || rc=$?
  distinct=$(grep -oE "[0-9,]+ distinct states found" "$out/logs/$name.log" | tail -1)
  if grep -qF "$expect" "$out/logs/$name.log"; then
    echo "$name: ok (expected: $expect; exit $rc; $distinct)" >> "$out/results.txt"
  else
    holds=false
    echo "$name: FAILED (expected: $expect; exit $rc; $distinct)" >> "$out/results.txt"
    items="$items{\"id\":\"$name\",\"artifact\":\"logs/$name.log\"},"
    cp "$out/logs/$name.log" "$out/counterexamples/$name.log"
  fi
done <<'MODELS'
Epoch:MCEpoch:No error has been found
EpochWrap:MCEpochWrap:Invariant NoLostWakeup is violated
Credit:MCCredit:No error has been found
CreditStale:MCCreditFull:Invariant StaleFull is violated
Abort:MCAbort:No error has been found
AbortEarly:MCAbortEarly:Temporal properties were violated
MODELS
items="[${items%,}]"
printf '{"holds":%s,"gaps":[{"id":"not-modelled","why":"memory orderings weaker than SeqCst; timeouts of sleep_on; the kernel futex beyond compare-and-sleep; MPI progress beyond weak fairness; two routes per node; lossy routes; process exit and MPI_Finalize; the relay out thread may scan at any time in the model, a superset of its wakes. See proofs/README.md"}],"counterexamples":%s}\n' "$holds" "$items" > "$out/outcome.json"
rm -rf "$work"
cat "$out/results.txt"
[ "$holds" = true ]
