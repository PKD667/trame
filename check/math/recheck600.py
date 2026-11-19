# Recomputes sampled rows of check/math/vectors.tsv at 600 bits and compares the rounded result bit for bit.
# Run from the repository root: nix develop "path:$PWD#math" -c python3 check/math/recheck600.py
import random, sys
from mpmath import mp, mpf
src = open("check/math/vectors.py").read().split("out = sys.stdout")[0]   # generator's definitions only
exec(src)
rows = [l.rstrip("\n").split("\t") for l in open("check/math/vectors.tsv")]
rng = random.Random(12345)
sel = rng.sample(range(len(rows)), 3000)
sel += [i for i, r in enumerate(rows) if r[0] in ("exp", "ln", "cbrt", "atanh", "sin")][:500]
bad = 0
for i in sel:
    r = rows[i]
    args = [flt(int(h, 16)) for h in r[1:-1]]
    mp.prec = 600
    got = round_f64(fns[r[0]][1](*[mpf(a) for a in args]))
    mp.prec = 400
    if got != int(r[-1], 16):
        bad += 1
        print("MISMATCH", r)
print("600-bit recheck: rows=%d mismatches=%d" % (len(sel), bad))
sys.exit(1 if bad else 0)
