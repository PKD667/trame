#!/usr/bin/env python3
# Writes check/math/vectors.tsv: inputs of each trame::math::f64 function with the CORRECTLY ROUNDED result.
# The reference is mpmath at 400 bits, rounded once to binary64 (nearest, ties to even, subnormals and
# overflow included, by integer arithmetic below). It does not use libm or Rust std, so the tests built
# on it do not inherit their errors (Rust's atanh, acosh are formulas that lose ~1000 ulp near the ends).
#
#   nix develop .#math -c python3 check/math/vectors.py > check/math/vectors.tsv
#
# Line: name TAB arg bits (16 hex) [TAB arg bits ...] TAB result bits. Deterministic: the seed is fixed.
import math, random, struct, sys
from mpmath import mp, mpf

mp.prec = 400
R = random.Random(0x7a4e_c0de)
N = 512  # inputs per regime (512 keeps every function above 1000 rows and the file near 2 MB)


def bits(x): return struct.unpack(">Q", struct.pack(">d", x))[0]
def flt(b): return struct.unpack(">d", struct.pack(">Q", b))[0]


def round_f64(v):
    """The binary64 nearest to the mpf `v`, ties to even, as bits."""
    if v == 0: return 0
    sign, man, exp, bc = v._mpf_
    if man == 0: return 0
    if exp is None or isinstance(exp, str): raise ValueError("non-finite reference")
    if not isinstance(v, mpf): raise TypeError("complex reference: %r" % (v,))
    e = exp + bc - 1               # |v| in [2^e, 2^(e+1))
    q = max(e - 52, -1074)         # exponent of the last place kept
    shift = q - exp
    if shift > 0:
        n, rem, half = man >> shift, man & ((1 << shift) - 1), 1 << (shift - 1)
        if rem > half or (rem == half and n & 1): n += 1
    else:
        n = man << -shift
    if n == 0: return (1 << 63) if sign else 0
    if n >> 53: n, q = n >> 1, q + 1   # carried to 2^53
    if q + n.bit_length() > 1024:              # n*2^q >= 2^1024: overflows to infinity
        return ((1 << 63) if sign else 0) | (0x7ff << 52)
    out = math.ldexp(n, q)                    # exact: n < 2^53 and the value is finite
    return bits(-out if sign else out)


def uniform(a, b): return lambda: R.uniform(a, b)
def logu(lo, hi, signed=False):
    def f():
        x = 2.0 ** R.uniform(lo, hi)
        return -x if signed and R.random() < 0.5 else x
    return f
def anybits():
    while True:
        x = flt(R.getrandbits(64))
        if math.isfinite(x): return x
def posbits():
    return abs(anybits())
def near_mult(step):                      # nearest double to k*step, moved by up to 3 ulps
    def f():
        k = R.randint(1, 10**6)
        return flt(bits(float(k) * step) + R.randint(-3, 3))
    return f

ONE = mpf(1)
fns = {
    # name: (arity, reference, [samplers])
    "exp":   (1, mp.exp, [uniform(-745.2, 709.8), logu(-60, 0, True), uniform(-1, 1)]),
    "exp_m1": (1, mp.expm1, [uniform(-40, 40), logu(-60, 0, True), uniform(-1, 1), uniform(700, 710)]),
    "ln":    (1, mp.log, [posbits, logu(-1, 1), lambda: 1 + R.uniform(-1e-3, 1e-3), logu(-1074, 1023)]),
    "ln_1p": (1, mp.log1p, [uniform(-1, 1), logu(-60, 0, True), logu(0, 1000), lambda: -1 + 2.0 ** R.uniform(-52, 0)]),
    "log2":  (1, lambda x: mp.log(x) / mp.log(2), [posbits, logu(-1, 1), lambda: 1 + R.uniform(-1e-3, 1e-3)]),
    "log10": (1, mp.log10, [posbits, logu(-1, 1), lambda: 1 + R.uniform(-1e-3, 1e-3)]),
    "exp2":  (1, lambda x: mp.power(2, x), [uniform(-1080, 1030), uniform(-1, 1), logu(-60, 0, True)]),
    "sqrt":  (1, mp.sqrt, [posbits, logu(-3, 3)]),
    "cbrt":  (1, lambda x: mp.cbrt(x) if x >= 0 else -mp.cbrt(-x), [anybits, logu(-3, 3, True)]),
    "powf":  (2, lambda x, y: mp.power(x, y), [(logu(-20, 20), uniform(-30, 30)), (uniform(0.5, 1.5), uniform(-100, 100)), (logu(-3, 3), logu(-3, 3, True))]),
    "hypot": (2, lambda x, y: mp.sqrt(x * x + y * y), [(logu(-1000, 1000, True), logu(-1000, 1000, True)), (logu(-3, 3, True), logu(-3, 3, True)), (anybits, anybits)]),
    "sin":   (1, mp.sin, [uniform(-0.8, 0.8), uniform(-1e6, 1e6), anybits, near_mult(math.pi / 2), logu(-60, 60, True)]),
    "cos":   (1, mp.cos, [uniform(-0.8, 0.8), uniform(-1e6, 1e6), anybits, near_mult(math.pi / 2), logu(-60, 60, True)]),
    "tan":   (1, mp.tan, [uniform(-0.8, 0.8), uniform(-1e6, 1e6), anybits, near_mult(math.pi / 2), logu(-60, 60, True)]),
    "atan":  (1, mp.atan, [uniform(-3, 3), logu(-60, 60, True), anybits]),
    "asin":  (1, mp.asin, [uniform(-1, 1), logu(-60, 0, True), lambda: math.copysign(1 - 2.0 ** R.uniform(-53, -1), R.random() - 0.5)]),
    "acos":  (1, mp.acos, [uniform(-1, 1), logu(-60, 0, True), lambda: math.copysign(1 - 2.0 ** R.uniform(-53, -1), R.random() - 0.5)]),
    "atan2": (2, lambda y, x: mp.atan2(y, x), [(uniform(-3, 3), uniform(-3, 3)), (logu(-500, 500, True), logu(-500, 500, True)), (anybits, anybits)]),
    "sinh":  (1, mp.sinh, [uniform(-5, 5), logu(-60, 0, True), uniform(-711, 711)]),
    "cosh":  (1, mp.cosh, [uniform(-5, 5), logu(-60, 0, True), uniform(-711, 711)]),
    "tanh":  (1, mp.tanh, [uniform(-5, 5), logu(-60, 0, True), uniform(-30, 30)]),
    "asinh": (1, mp.asinh, [uniform(-5, 5), logu(-60, 0, True), logu(0, 1000, True)]),
    "acosh": (1, mp.acosh, [lambda: 1 + 2.0 ** R.uniform(-52, 0), uniform(1, 5), logu(0, 1000)]),
    "atanh": (1, mp.atanh, [uniform(-1, 1), logu(-60, -1, True), lambda: math.copysign(1 - 2.0 ** R.uniform(-53, -1), R.random() - 0.5)]),
}

# Inputs where the answer is exact or an edge: always included.
EDGES = {
    "exp2": [-1074.0, -1022.0, -1075.0, 1023.0, 1023.9999999999998, 1024.0, 0.0, 1.0, 10.0, -10.0, 0.5],
    "log2": [1.0, 2.0, 8.0, 0.5, 1024.0, 5e-324, 2.2250738585072014e-308],
    "log10": [1.0, 10.0, 1000.0, 1e22, 5e-324],
    "cbrt": [8.0, -8.0, 27.0, 1e-300, 5e-324, 1.7976931348623157e308],
    "sqrt": [4.0, 2.0, 5e-324, 1.7976931348623157e308],
    "exp": [0.0, 1.0, 709.782712893384, 709.7827128933841, -745.1332191019411, -745.1332191019412, 5e-324],
    "ln": [1.0, 2.0, 5e-324, 1.7976931348623157e308],
}

def valid(name, args):
    x = args[0]
    if name in ("ln", "log2", "log10", "sqrt") and not x > 0: return False
    if name == "ln_1p" and not x > -1: return False
    if name == "acosh" and not x >= 1: return False
    if name in ("asin", "acos", "atanh") and not abs(x) < 1: return False
    if name == "powf" and not (args[0] > 0): return False
    if name in ("sin", "cos", "tan") and x == 0: return False
    return True

out = sys.stdout
skipped = {}
for name, (arity, ref, samplers) in fns.items():
    rows = [(x,) for x in EDGES.get(name, [])]
    for s in samplers:
        for _ in range(N):
            rows.append(tuple(f() for f in s) if arity > 1 else (s(),))
    for args in rows:
        if not valid(name, args):
            skipped[name] = skipped.get(name, 0) + 1
            continue
        v = ref(*[mpf(a) for a in args])          # any exception here is a bug: let it crash
        if not mp.isfinite(v):
            skipped[name] = skipped.get(name, 0) + 1
            continue                              # NaN/inf reference: the special-value tests own those
        res = round_f64(v)
        out.write("\t".join([name] + ["%016x" % bits(a) for a in args] + ["%016x" % res]) + "\n")
for name, k in skipped.items():
    print("vectors.py: %s: %d rows skipped (outside the domain)" % (name, k), file=sys.stderr)
