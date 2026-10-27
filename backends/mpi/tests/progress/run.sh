#!/usr/bin/env bash
# Credit-return matrix. Usage: bash run.sh OUTROOT   (needs nix; builds progress.c with mpicc)
# Each configuration writes its own directory; the OSC component selected is recorded from the verbose log.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
root="${1:?usage: run.sh OUTROOT}"
K="${K:-200}"; CAP="${CAP:-4}"; STALL="${STALL:-200}"
mkdir -p "$root"
cd "$here"
nix develop "$here/../../../..#mpi" -c bash -c '
  set -u
  mpicc -O2 -Wall -o "$0/progress" progress.c
  for cfg in "shm|" "rma|--mca osc sm" "rma|--mca osc rdma" "acc|--mca osc sm" "acc|--mca osc rdma"; do
    mode=${cfg%%|*}; opt=${cfg#*|}
    tag=$(echo "$mode-$opt" | tr -d " " | tr "-" "_")
    dir="$0/$tag"; mkdir -p "$dir"
    timeout 900 mpirun --oversubscribe -n 2 $opt --mca osc_base_verbose 100 "$0/progress" $mode '"$CAP $K $STALL"' "$dir" > "$dir/mpirun.log" 2>&1
    echo "$tag exit=$? osc=$(grep -o "component [a-z]* selected" "$dir/mpirun.log" | sort -u | tr "\n" " ")"
  done' "$root" 2>&1 | grep -v "^warning: Git"
