//! Fused multiply-add, correctly rounded (IEEE 754 binary64), in integer arithmetic only.
//!
//! `mul_add(a, b, c)` is `a*b + c` with one rounding. The method:
//! 1. The product of the two 53-bit mantissas is exact in `u128` (at most 106 bits).
//! 2. The product and `c` are each normalised: shifted left so the leading bit is bit 126 of a `u128`.
//!    A left shift loses nothing. Afterwards each has at least 21 trailing zero bits.
//! 3. The operand with the smaller exponent is shifted right to the other's exponent. Bits shifted
//!    out are folded into bit 0 as a sticky bit. An operand with 21 trailing zeros loses no bit to a
//!    shift of 21 or less, so the sticky bit is set only after a shift of 22 or more. Then the
//!    larger operand is at least 2^126 and the sum or difference keeps at least 125 bits above
//!    bit 0, so bit 0 lies at least 73 places below the rounding position.
//! 4. Add, or subtract the smaller magnitude from the larger (exact, in `u128`).
//! 5. Round once to nearest, ties to even: 53 bits, or fewer when the result is subnormal. The
//!    shift for a subnormal result is taken before rounding.
//! 6. Pack. A result at or above 2^1024 is infinite.
//!
//! Specials come first: NaN, `inf * 0`, infinities, and zero operands. A zero result from nonzero
//! operands is `+0` (round to nearest), as is `-0 * 1 + +0`; `-0` only when product and `c` are both `-0`.
//! Checked against std's `f64::mul_add` in `src/math/f64/tests/fma.rs`.

const FRAC_BITS: u32 = 52;
const HIDDEN: u64 = 1 << FRAC_BITS;
const SIGN: u64 = 1 << 63;

/// `(negative, k, m)` with `|x| = m * 2^k` for finite `x`. `m` is 0 exactly for zeros.
/// A subnormal `m` is not normalised here; `normalise` does that.
fn unpack(x: f64) -> (bool, i64, u64) {
    let bits = x.to_bits();
    let neg = bits & SIGN != 0;
    let be = ((bits >> FRAC_BITS) & 0x7ff) as i64;
    let frac = bits & (HIDDEN - 1);
    if be == 0 {
        (neg, -1074, frac)
    } else {
        (neg, be - 1075, frac | HIDDEN)
    }
}

/// `m * 2^k` with `m` moved so its leading bit is bit 126 (`m` nonzero, below 2^127). Exact.
fn normalise(m: u128, k: i64) -> (u128, i64) {
    let s = m.leading_zeros() - 1;
    (m << s, k - i64::from(s))
}

/// `s` shifted right by `drop` (at least 1), rounded to nearest, ties to even. Bits below the
/// rounding bit, including the sticky bit 0, decide whether a tie is broken.
fn round_even(s: u128, drop: u32) -> u128 {
    let (q, half, rest) = if drop > 128 {
        (0, false, s != 0)
    } else if drop == 128 {
        (0, s >> 127 != 0, s & ((1_u128 << 127) - 1) != 0)
    } else {
        (s >> drop, (s >> (drop - 1)) & 1 != 0, s & ((1_u128 << (drop - 1)) - 1) != 0)
    };
    if half && (rest || q & 1 != 0) {
        q + 1
    } else {
        q
    }
}

fn signed_zero(neg: bool) -> f64 {
    f64::from_bits(if neg { SIGN } else { 0 })
}

fn signed_inf(neg: bool) -> f64 {
    if neg {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    }
}

/// Round `s * 2^k` to binary64 and pack it with sign `neg`. `s` is nonzero; bit 0 may be sticky.
fn round_pack(s: u128, k: i64, neg: bool) -> f64 {
    let top = 127 - i64::from(s.leading_zeros()); // leading bit of s
    let e = k + top; // the value lies in [2^e, 2^(e+1))
    let lsb = (e - i64::from(FRAC_BITS)).max(-1074); // exponent of the last bit kept
    let drop = lsb - k; // bits of s below `lsb`; at most 0 means none
    let (mut q, mut lsb) = if drop <= 0 { (s << (-drop) as u32, lsb) } else { (round_even(s, drop as u32), lsb) };
    if q == 1_u128 << (FRAC_BITS + 1) {
        q >>= 1;
        lsb += 1;
    }
    if q == 0 {
        return signed_zero(neg);
    }
    let sign = if neg { SIGN } else { 0 };
    if q >= 1_u128 << FRAC_BITS {
        // value = q * 2^lsb with q in [2^52, 2^53): biased exponent = lsb + 52 + 1023.
        let biased = lsb + i64::from(FRAC_BITS) + 1023;
        if biased >= 2047 {
            return signed_inf(neg);
        }
        let bits = ((biased as u64) << FRAC_BITS) | (q as u64 - HIDDEN);
        f64::from_bits(bits | sign)
    } else {
        // Subnormal: the only case where the kept bits do not fill 53 places, so lsb is -1074.
        debug_assert_eq!(lsb, -1074);
        f64::from_bits(q as u64 | sign)
    }
}

/// Fused multiply-add: `a*b + c` with one correct rounding, bit-identical to IEEE 754.
pub fn mul_add(a: f64, b: f64, c: f64) -> f64 {
    if a.is_nan() || b.is_nan() || c.is_nan() {
        return f64::NAN;
    }
    let (neg_a, ka, ma) = unpack(a);
    let (neg_b, kb, mb) = unpack(b);
    let (neg_c, kc, mc) = unpack(c);
    let neg_p = neg_a != neg_b;

    // Specials. A finite zero has m == 0; an infinity has m == HIDDEN, so the tests below are exact.
    if a.is_infinite() || b.is_infinite() {
        if ma == 0 || mb == 0 {
            return f64::NAN; // inf * 0
        }
        if c.is_infinite() && neg_c != neg_p {
            return f64::NAN; // inf - inf
        }
        return signed_inf(neg_p);
    }
    if c.is_infinite() {
        return c;
    }
    if ma == 0 || mb == 0 {
        // The product is a signed zero. Zero sum: -0 only if the product and c are both -0.
        if mc == 0 {
            return signed_zero(neg_p && neg_c);
        }
        return c;
    }
    if mc == 0 {
        // c is zero and the product is nonzero: one IEEE multiplication rounds it correctly.
        return a * b;
    }

    // Everything finite and nonzero. The product is exact; c is an integer mantissa.
    let (p, kp) = normalise(u128::from(ma) * u128::from(mb), ka + kb);
    let (cm, kc) = normalise(u128::from(mc), kc);

    // The larger exponent is x. Shift y right to x's exponent, folding lost bits into bit 0.
    let (x, kx, neg_x, y, ky, neg_y) =
        if kp >= kc { (p, kp, neg_p, cm, kc, neg_c) } else { (cm, kc, neg_c, p, kp, neg_p) };
    let d = (kx - ky) as u32;
    let (ys, sticky) = if d >= 128 {
        (0, true) // y is nonzero, so everything is lost
    } else {
        (y >> d, y & ((1_u128 << d) - 1) != 0)
    };
    let y = ys | u128::from(sticky);

    let (s, neg) = if neg_x == neg_y {
        (x + y, neg_x) // below 2^128: both are below 2^127
    } else if x >= y {
        (x - y, neg_x)
    } else {
        (y - x, neg_y) // only when the exponents are equal, so no bit was lost
    };
    if s == 0 {
        return 0.0; // exact cancellation, round to nearest: +0
    }
    round_pack(s, kx, neg)
}
