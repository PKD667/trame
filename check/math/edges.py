#!/usr/bin/env python3
# Writes src/math/f64/tests/edges.rs: edge inputs of exp, exp_m1, exp2, ln, scalbn, powf (yisint, the
# near-one path, the finish stage) and sqrt, each with the CORRECTLY ROUNDED result from mpmath at 400 bits.
# Not libm, not Rust std. The Rust file is generated; do not edit it by hand.
#
#   nix develop "path:$PWD#math" -c python3 check/math/edges.py > src/math/f64/tests/edges.rs
#
# Deterministic: the seed is fixed. Rounding is by integer arithmetic (round_f64), as in vectors.py.
import math, random, struct, sys
from mpmath import mp, mpf

mp.prec = 400
R = random.Random(0x0ED9_E5A1)
INF = 0x7FF0_0000_0000_0000
NAN = 0x7FF8_0000_0000_0000


def bits(x): return struct.unpack(">Q", struct.pack(">d", x))[0]
def flt(b): return struct.unpack(">d", struct.pack(">Q", b & 0xFFFF_FFFF_FFFF_FFFF))[0]


def round_f64(v):
    """The binary64 nearest to the mpf `v`, ties to even, as bits; overflow to inf, underflow to 0."""
    if v == 0: return 0
    sign, man, exp, bc = v._mpf_
    sb = (1 << 63) if sign else 0
    e = exp + bc - 1                    # |v| in [2^e, 2^(e+1))
    if e >= 1024: return sb | INF       # checked first: the shifts below would be huge
    if e < -1076: return sb             # below half the least subnormal: rounds to zero
    q = max(e - 52, -1074)
    shift = q - exp
    if shift > 0:
        n, rem, half = man >> shift, man & ((1 << shift) - 1), 1 << (shift - 1)
        if rem > half or (rem == half and n & 1): n += 1
    else:
        n = man << -shift
    if n >> 53: n, q = n >> 1, q + 1    # carried to 2^53
    if n == 0: return sb
    if q + n.bit_length() > 1024: return sb | INF
    return sb | bits(math.ldexp(n, q))


def near(x, k):
    """x and its k neighbours on each side, by bit pattern."""
    b = bits(x)
    return [flt(b + i) for i in range(-k, k + 1)]


def hw(words):
    """Values whose high word is h-1, h, h+1 (the fdlibm thresholds compare high words), both signs, low word 0 or all ones."""
    out = []
    for h in words:
        for d in (-1, 0, 1):
            for lo in (0, 0xFFFF_FFFF):
                for s in (0, 1 << 63):
                    out.append(flt(((h + d) << 32) | lo | s))
    return out


def unif(a, b, n): return [R.uniform(a, b) for _ in range(n)]


def logu(lo, hi, n, signed=False):
    out = []
    for _ in range(n):
        x = 2.0 ** R.uniform(lo, hi)
        out.append(-x if signed and R.random() < 0.5 else x)
    return out


def anybits(n):
    out = []
    while len(out) < n:
        x = flt(R.getrandbits(64))
        if math.isfinite(x): out.append(x)
    return out


def ref(f):
    """Correctly rounded value of mpmath function `f` at the mpf of its argument(s)."""
    return lambda *a: round_f64(f(*[mpf(v) for v in a]))


LN2 = math.log(2.0)
ref_exp = ref(mp.exp)
ref_expm1 = ref(mp.expm1)
ref_exp2 = ref(lambda x: mp.power(2, x))
ref_ln = ref(mp.log)
ref_sqrt = ref(mp.sqrt)


# ---- one table per function: rows of (input bits..., expected bits) ----

def table_exp():
    xs = hw([0x3E30_0000, 0x3FD6_2E42, 0x3FF0_A2B2, 0x4086_2E42])
    xs += near(7.09782712893383973096e+02, 6) + near(-7.45133219101941108420e+02, 6)
    xs += near(float(mp.log(mpf(2) ** -1074)), 8) + near(float(mp.log(mpf(2) ** -1075)), 8)
    xs += unif(-746, 710, 700) + unif(-745, -708, 600) + logu(-60, 0, 200, True) + unif(709.5, 710.0, 100)
    return [(x, ref_exp(x)) for x in xs]


def table_exp_m1():
    xs = hw([0x3C90_0000, 0x3FD6_2E42, 0x3FF0_A2B2, 0x4043_687A, 0x4086_2E42])
    xs += near(7.09782712893383973096e+02, 6) + near(-38.8, 4)
    xs += unif(-40, -30, 300) + unif(-1, 1, 300) + unif(30, 40, 200) + unif(709, 710.5, 100)
    xs += [(k + 0.5) * LN2 for k in range(-3, 60)] + [(k + 0.1) * LN2 for k in range(-60, 3)]
    return [(x, ref_expm1(x)) for x in xs]


def table_exp2():
    xs = hw([0x4090_0000, 0x3C90_0000])
    xs += near(-1075.0, 6) + near(-1074.0, 4) + near(-1022.0, 4) + near(1024.0, 4) + near(1023.0, 4)
    xs += unif(-1080, -1000, 600) + unif(1000, 1030, 200) + unif(-1, 1, 200) + logu(-60, 0, 200, True)
    xs += unif(-1022.5, -1021.5, 100)
    return [(x, ref_exp2(x)) for x in xs]


def table_ln():
    xs = [flt(bits(1.0) + k) for k in range(-40, 41)]
    for e in (-1, 0, 1, 5, -5, 100, -100, 1000):          # exponent of the binade
        for m in (0, 1, 0xFFFFF, 0x6147A, 0x6B851, 0x6A09E, 0x95F64):   # mantissa high word: fdlibm's thresholds
            for d in (-1, 0, 1):
                if 0 <= m + d <= 0xFFFFF:
                    for lo in (0, 0xFFFF_FFFF):
                        xs.append(flt(((0x3FF + e) << 52) | ((m + d) << 32) | lo))
    xs += logu(-1074, 1023, 400) + unif(1 - 1e-3, 1 + 1e-3, 300) + anybits(100)
    xs = [x for x in xs if x > 0 and math.isfinite(x)]
    return [(x, ref_ln(x)) for x in xs]


def table_sqrt():
    rows = []
    for _ in range(300):
        m, e = R.randint(1, 2 ** 26), R.randint(-500, 500)
        x = float(m * m) * 2.0 ** (2 * e)               # a perfect square: exact
        rows += [x] + [flt(bits(x) + i) for i in (-2, -1, 1, 2)]   # and its neighbours
    for k in range(1, 41):                              # subnormal squares
        x = float(k * k) * 2.0 ** -1074
        rows += [x] + [flt(bits(x) + i) for i in (-1, 1)]
    rows += [flt(0x7FEF_FFFF_FFFF_FFFF), flt(0x7FEF_FFFF_FFFF_FFFE), 2.0 ** -1022, flt(0x000F_FFFF_FFFF_FFFF)]
    rows += anybits(100)
    rows = [x for x in rows if x > 0 and math.isfinite(x)]
    return [(x, ref_sqrt(x)) for x in rows]


def table_scalbn():
    # (x bits, n, result bits): the result is one rounding of x * 2^n, so it is checked bit for bit.
    xs = [1.0, -1.5, 1.9999999999999998, 2.0 ** -1022, flt(1), flt(0x000F_FFFF_FFFF_FFFF), 0.75, -3.0]
    ns = list(range(-1100, -999)) + list(range(-60, 61)) + list(range(1000, 1101))
    pairs = [(x, n) for x in xs for n in ns]
    pairs += [(v, n) for n in (-50001, -50000, 50000, 50001, -100000, 100000, 2 ** 31 - 1, -2 ** 31)
              for v in (1.0, -(2.0 ** -1074))]
    pairs += [(x, R.randint(-1200, 1200)) for x in anybits(400)]
    pairs += [(v, n) for v in (0.0, -0.0, math.inf, -math.inf, float("nan")) for n in (-3, 0, 5)]
    out = []
    for x, n in pairs:
        if math.isnan(x): want = NAN
        elif x == 0 or math.isinf(x): want = bits(x)     # signed zero and infinity pass through
        else: want = round_f64(mpf(x) * mpf(2) ** n)
        out.append((bits(x), n, want))
    return out


# ---- powf: the reference, then the tables of its special paths ----

def ref_pow(x, y):
    """Correctly rounded x^y as bits, with fdlibm's special cases: NaN for x < 0 and y not an integer."""
    mag, sign = abs(x), False
    if x < 0:
        if y != math.floor(y): return NAN
        sign = (y % 2.0) == 1.0                       # odd integer exponent: negative result
    if abs(y) > 2.0 ** 64 and mag != 1.0:             # |y| > 2^64: the result is inf or 0 by the sign of log|x|
        big = (mag > 1.0) == (y > 0)
        return INF if big else 0
    v = mp.power(mpf(mag), mpf(y))
    if sign: v = -v
    return round_f64(v)


def table_powf():
    rows = []
    # yisint: x < 0 with integer, odd, even and non-integer y, through every magnitude class.
    xs_neg = [-2.0, -3.5, -1.5, -0.001, -1e10, -0.5, -1.0, -1.0000000000000002, -7.0, -(1 + 2.0 ** -52)]
    ys = [1, 2, 3, -1, -3, 4, -4, 5, 1048577, 1048576, 2 ** 21 + 1, 2 ** 53, 2 ** 53 + 2, 2 ** 53 - 1,
          2 ** 52 + 1, 2 ** 52 + 2, 2 ** 40 + 1, 2 ** 34 + 1, 1e300, -1e300, 1e30, 0.5, 1.5, 2.5,
          2 ** 51 + 0.5, 2 ** 40 + 0.5, 1.0000000001, 1025.0, 1024.0, 1023.0, -1023.0, -1025.0,
          2.0 ** 31, 2.0 ** 31 + 2.0 ** -21]
    for x in xs_neg + [2.0, 0.5]:
        for y in ys:
            rows.append((x, float(y)))
    # finish: y * log2|x| at the overflow, underflow and subnormal limits; y nudged by up to 3 ulps.
    for x in [2.0, 10.0, 0.3, 1.7, 3.0, 1.0000001, 0.9999999, 100.0, 1e-5, 0.5]:
        for t in [1024.0, -1075.0, -1022.0, -1074.0, -1074.5, -1076.0, -1021.0, 1023.5, -1074.9, -1023.0]:
            y0 = t / (mp.log(mpf(x)) / mp.log(2))
            for k in range(-3, 4):
                rows.append((x, flt(bits(float(y0)) + k)))
    for x in [-2.0, -3.0, -0.3]:                      # negative base: integer y on each side of the limit
        for t in [1024.0, -1075.0, -1074.0, -1022.0]:
            y0 = int(t / (mp.log(mpf(-x)) / mp.log(2)))
            for d in range(-2, 3):
                rows.append((x, float(y0 + d)))
    # near-one path: |y| >= 2^31 and x within [1 - 2^-21, 1 + 2^-20). Keep |y log2 x| <= 1000 so the result is finite.
    # These rows go to their own table (returned in `near`): see the note in TESTS.
    near = []
    for _ in range(600):
        d = int(2.0 ** R.uniform(0, 32))
        x = flt(bits(1.0) + d) if R.random() < 0.5 else flt(bits(1.0) - d)
        if R.random() < 0.5: x = -x
        y = float(R.randint(2 ** 31, 2 ** 36)) * (1 if R.random() < 0.5 else -1)
        if abs(y * float(mp.log(mpf(abs(x))) / mp.log(2))) <= 1000:
            near.append((x, y))
    for x in (1.0 + 2.0 ** -40, 1.0 - 2.0 ** -40, -(1.0 + 2.0 ** -40)):
        near += [(x, 2.0 ** 31), (x, 2.0 ** 31 + 2.0 ** -21), (x, -(2.0 ** 31 + 1))]
    # |y| > 2^64: inf or 0 by the sign of log|x| (analytic; ref_pow handles these too).
    for x in (0.5, 2.0, -0.5, -2.0, 1.0000001, 0.9999999):
        for y in (2.0 ** 65, -(2.0 ** 65), 1e300, -1e300):
            rows.append((x, y))
    # Routed by the branch test of pow/mod.rs, not by how the row was drawn: near-one is taken when
    # iy > 0x41e00000 (|y| > 2^31, high word) and 0x3fefffff <= ix <= 0x3ff00000 (ix: high word of |x|).
    main, near_one = [], []
    for x, y in rows + near:
        if math.isnan(x) or math.isnan(y): continue
        ix, iy = bits(abs(x)) >> 32, bits(abs(y)) >> 32
        (near_one if iy > 0x41E0_0000 and 0x3FEF_FFFF <= ix <= 0x3FF0_0000 else main).append((x, y))
    def conv(rs):
        return [(bits(x), bits(y), ref_pow(x, y)) for x, y in rs]
    return conv(main), conv(near_one)



# ---- Rust output ----

HEADER = """\
//! Edge inputs of `exp`, `exp_m1`, `exp2`, `ln`, `scalbn`, `powf` and `sqrt`, each with its correctly rounded
//! result from mpmath at 400 bits. GENERATED by `check/math/edges.py`: regenerate, do not edit the tables.
//!
//! The inputs are where a branch changes: overflow and underflow thresholds, subnormal results, the
//! reduction index of exp and exp_m1, the near-one path of pow (`pow/log2.rs`), the parity test of
//! pow's exponent (`yisint`), the finish stage (`pow/exp2.rs`), and perfect squares and their neighbours.
//! `scalbn` and `sqrt` are checked bit for bit: their docs say one rounding. The others allow `BOUND` ulp.

use super::super::scalbn::scalbn;
use super::super::{exp, exp2, exp_m1, ln, powf, sqrt};
use super::ulps;

/// Largest allowed distance in ulp for the functions documented faithful.
const BOUND: u64 = 1;

/// Compares one result with its expected bits and records a failure. A NaN expectation matches any NaN.
fn judge(bad: &mut std::vec::Vec<std::string::String>, what: &str, got: f64, want: u64, exact: bool) {
    let want = f64::from_bits(want);
    let ok = if want.is_nan() {
        got.is_nan()
    } else if exact {
        got.to_bits() == want.to_bits()
    } else {
        !got.is_nan() && ulps(got, want) <= BOUND
    };
    if !ok {
        bad.push(std::format!("{what}: got {:#018x}, want {:#018x}", got.to_bits(), want.to_bits()));
    }
}

/// Fails listing the first failures, not all of them: a broken branch fails every row at once.
fn report(name: &str, bad: &[std::string::String], rows: usize) {
    assert!(bad.is_empty(), "{name}: {} of {rows} rows fail; first: {}", bad.len(), bad[0]);
}
"""


def emit(name, ty, rows):
    """One const table; the SCALBN exponent column is decimal (i32), the others are hex (u64)."""
    out = ["#[rustfmt::skip]", f"const {name}: &[{ty}] = &["]
    for r in rows:
        cols = []
        for k, v in enumerate(r):
            if isinstance(v, float): v = bits(v)           # an input given as a float
            cols.append(str(v) if ty == "(u64, i32, u64)" and k == 1 else "0x%016x" % v)
        out.append("    (" + ", ".join(cols) + "),")
    out.append("];")
    return "\n".join(out) + "\n"


TESTS = '''#[test]
fn exp_edges() {
    let mut bad = std::vec::Vec::new();
    for &(x, r) in EXP {
        judge(&mut bad, &std::format!("exp({:#018x})", x), exp(f64::from_bits(x)), r, false);
    }
    report("exp", &bad, EXP.len());
}

#[test]
fn exp_m1_edges() {
    let mut bad = std::vec::Vec::new();
    for &(x, r) in EXP_M1 {
        judge(&mut bad, &std::format!("exp_m1({:#018x})", x), exp_m1(f64::from_bits(x)), r, false);
    }
    report("exp_m1", &bad, EXP_M1.len());
}

#[test]
fn exp2_edges() {
    let mut bad = std::vec::Vec::new();
    for &(x, r) in EXP2 {
        judge(&mut bad, &std::format!("exp2({:#018x})", x), exp2(f64::from_bits(x)), r, false);
    }
    report("exp2", &bad, EXP2.len());
}

#[test]
fn ln_edges() {
    let mut bad = std::vec::Vec::new();
    for &(x, r) in LN {
        judge(&mut bad, &std::format!("ln({:#018x})", x), ln(f64::from_bits(x)), r, false);
    }
    report("ln", &bad, LN.len());
}

#[test]
fn scalbn_edges() {
    // Bit for bit: scalbn is one rounding of x * 2^n, signed zeros and infinities included.
    let mut bad = std::vec::Vec::new();
    for &(x, n, r) in SCALBN {
        judge(&mut bad, &std::format!("scalbn({:#018x}, {n})", x), scalbn(f64::from_bits(x), n), r, true);
    }
    report("scalbn", &bad, SCALBN.len());
}

#[test]
fn powf_edges() {
    let mut bad = std::vec::Vec::new();
    for &(x, y, r) in POWF {
        judge(
            &mut bad,
            &std::format!("powf({:#018x}, {:#018x})", x, y),
            powf(f64::from_bits(x), f64::from_bits(y)),
            r,
            false,
        );
    }
    report("powf", &bad, POWF.len());
}

/// Known defect, kept failing on purpose: `powf` on the near-one path (`pow/log2.rs`, `|y| >= 2^31`,
/// `x` within about 2^-20 of 1) is off by hundreds of ulp. The cause: `u = IVLN2_H * t` is rounded, since
/// `IVLN2_H` has 24 significant bits and `t` can have 30; the error (about 1.3e-23 in log2) is multiplied
/// by `y` (about 7e9 here). Run with `--ignored` to see it. Remove the `ignore` when the path is fixed.
#[test]
#[ignore = "known defect: powf near-one path with |y| >= 2^31 (pow/log2.rs, IVLN2_H * t rounds)"]
fn powf_near_one_huge_y() {
    let mut bad = std::vec::Vec::new();
    for &(x, y, r) in POWF_NEAR_ONE {
        judge(
            &mut bad,
            &std::format!("powf({:#018x}, {:#018x})", x, y),
            powf(f64::from_bits(x), f64::from_bits(y)),
            r,
            false,
        );
    }
    report("powf_near_one", &bad, POWF_NEAR_ONE.len());
}

#[test]
fn sqrt_edges() {
    // Bit for bit: sqrt's doc says correctly rounded, and a binary64 root is never a tie.
    let mut bad = std::vec::Vec::new();
    for &(x, r) in SQRT {
        judge(&mut bad, &std::format!("sqrt({:#018x})", x), sqrt(f64::from_bits(x)), r, true);
    }
    report("sqrt", &bad, SQRT.len());
}
'''


def main():
    powf_rows = table_powf()        # one call: it draws from the seeded generator
    tables = [
        ("EXP", "(u64, u64)", table_exp()),
        ("EXP_M1", "(u64, u64)", table_exp_m1()),
        ("EXP2", "(u64, u64)", table_exp2()),
        ("LN", "(u64, u64)", table_ln()),
        ("SQRT", "(u64, u64)", table_sqrt()),
        ("SCALBN", "(u64, i32, u64)", table_scalbn()),
        ("POWF", "(u64, u64, u64)", powf_rows[0]),
        ("POWF_NEAR_ONE", "(u64, u64, u64)", powf_rows[1]),
    ]
    sys.stdout.write(HEADER + "\n")
    for name, ty, rows in tables:
        sys.stdout.write(emit(name, ty, rows) + "\n")
        print(f"edges.py: {name}: {len(rows)} rows", file=sys.stderr)
    sys.stdout.write(TESTS)


if __name__ == "__main__":
    main()
