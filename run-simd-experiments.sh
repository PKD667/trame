#!/bin/bash
# Run SIMD benchmark on Grid'5000 across multiple CPU types on BESTEFFORT queue.
# Usage: TREE=/path/to/simd REMOTE=simd-exp ./run-simd-experiments.sh

set -e
here=$(cd "$(dirname "$0")" && pwd)
g5k="${here}/../../agents/trame-worker-model-20261027/g5k"

if [ ! -f "$g5k/host" ]; then
    echo "Error: cannot find Grid'5000 host file at $g5k/host" >&2
    exit 1
fi

TREE=${TREE:-"$here"}
REMOTE=${REMOTE:-wt-simd}
BENCH_NAME="simd-bench-$(date +%s)"

# The experiment targets. Format: name:cluster:cpu_description
# Lille clusters and their known CPU types:
# - chiclet: AMD EPYC 7301 (Zen 1)
# - chifflot: Intel Xeon Gold 5220 (no AVX-512)
# - gros: Intel Xeon Platinum 8260 (AVX-512)
# Nancy (if available):
# - grvingt: Intel Xeon Platinum 8260 (AVX-512)
# - grimoire: AMD EPYC 7002 (Zen 2)
targets=(
    "zen1:chiclet:AMD_EPYC_7301_Zen1"
    "intel-avx512:gros:Intel_Xeon_Platinum_8260_AVX512"
    "zen2plus:grimoire:AMD_EPYC_7002_Zen2"
)

for target in "${targets[@]}"; do
    IFS=: read name cluster cpu_desc <<< "$target"
    
    echo "=== Attempting to run on ${cluster} (${cpu_desc}) ==="
    
    # Try to create a BESTEFFORT job on the target cluster
    # This is a template; actual submission would depend on nixsci's provider setup
    # For now, we document what would be needed
    
    echo "Would run: nix run .#simd-bench-${name} on ${cluster}"
    # In practice, nixsci would handle this if the provider is configured
done

echo ""
echo "Note: Actual job submission requires nixsci providers configured in ~/.config/nixsci/providers.json"
echo "Available providers should include:"
echo "  - g5k-lille-zen1: Lille chiclet cluster"
echo "  - g5k-lille-xeon: Lille gros cluster"  
echo "  - g5k-besteffort-zen2: Nancy grimoire cluster (if available)"
