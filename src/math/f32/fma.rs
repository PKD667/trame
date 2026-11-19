//! Fused multiply-add for f32, correctly rounded (IEEE 754 binary32), in integer arithmetic only.
//!
//! The method is `src/math/f64/fma.rs`, with `u64` for the working integers: the product of two
//! 24-bit mantissas is exact in `u64` (at most 48 bits), and the normalised operands have leading
//! bit 62. Each normalised operand has at least 15 trailing zero bits (the product) or 39 (c), so
//! the sticky bit folded into bit 0 is safe by the same argument. Strict tests: `fma_tests.rs`.

const FRAC_BITS: u32 = 23;
const HIDDEN: u64 = 1 << FRAC_BITS;
const SIGN: u32 = 1 << 31;

/// `(negative, k, m)` with `|x| = m * 2^k` for finite `x`. `m` is 0 exactly for zeros.
fn unpack(x: f32) -> (bool, i64, u64) {
    let bits = x.to_bits();
    let neg = bits & SIGN != 0;
    let be = i64::from((bits >> FRAC_BITS) & 0xff);
    let frac = u64::from(bits & (HIDDEN as u32 - 1));
    if be == 0 {
        (neg, -149, frac)
    } else {
        (neg, be - 150, frac | HIDDEN)
    }
}

/// `m * 2^k` with `m` moved so its leading bit is bit 62 (`m` nonzero, below 2^63). Exact.
fn normalise(m: u64, k: i64) -> (u64, i64) {
    let s = m.leading_zeros() - 1;
    (m << s, k - i64::from(s))
}

/// `s` shifted right by `drop` (at least 1), rounded to nearest, ties to even.
fn round_even(s: u64, drop: u32) -> u64 {
    let (q, half, rest) = if drop > 64 {
        (0, false, s != 0)
    } else if drop == 64 {
        (0, s >> 63 != 0, s & ((1_u64 << 63) - 1) != 0)
    } else {
        (s >> drop, (s >> (drop - 1)) & 1 != 0, s & ((1_u64 << (drop - 1)) - 1) != 0)
    };
    if half && (rest || q & 1 != 0) {
        q + 1
    } else {
        q
    }
}

fn signed_zero(neg: bool) -> f32 {
    f32::from_bits(if neg { SIGN } else { 0 })
}

fn signed_inf(neg: bool) -> f32 {
    if neg {
        f32::NEG_INFINITY
    } else {
        f32::INFINITY
    }
}

/// Round `s * 2^k` to binary32 and pack it with sign `neg`. `s` is nonzero; bit 0 may be sticky.
fn round_pack(s: u64, k: i64, neg: bool) -> f32 {
    let top = 63 - i64::from(s.leading_zeros()); // leading bit of s
    let e = k + top; // the value lies in [2^e, 2^(e+1))
    let lsb = (e - i64::from(FRAC_BITS)).max(-149); // exponent of the last bit kept
    let drop = lsb - k; // bits of s below `lsb`; at most 0 means none
    let (mut q, mut lsb) = if drop <= 0 { (s << (-drop) as u32, lsb) } else { (round_even(s, drop as u32), lsb) };
    if q == 1_u64 << (FRAC_BITS + 1) {
        q >>= 1;
        lsb += 1;
    }
    if q == 0 {
        return signed_zero(neg);
    }
    let sign = if neg { SIGN } else { 0 };
    if q >= 1_u64 << FRAC_BITS {
        // value = q * 2^lsb with q in [2^23, 2^24): biased exponent = lsb + 23 + 127.
        let biased = lsb + i64::from(FRAC_BITS) + 127;
        if biased >= 255 {
            return signed_inf(neg);
        }
        let bits = ((biased as u32) << FRAC_BITS) | (q as u32 - HIDDEN as u32);
        f32::from_bits(bits | sign)
    } else {
        debug_assert_eq!(lsb, -149);
        f32::from_bits(q as u32 | sign)
    }
}

/// Fused multiply-add: `a*b + c` with one correct rounding, bit-identical to IEEE 754.
pub fn mul_add(a: f32, b: f32, c: f32) -> f32 {
    if a.is_nan() || b.is_nan() || c.is_nan() {
        return f32::NAN;
    }
    let (neg_a, ka, ma) = unpack(a);
    let (neg_b, kb, mb) = unpack(b);
    let (neg_c, kc, mc) = unpack(c);
    let neg_p = neg_a != neg_b;

    // Specials, as in the f64 version.
    if a.is_infinite() || b.is_infinite() {
        if ma == 0 || mb == 0 {
            return f32::NAN;
        }
        if c.is_infinite() && neg_c != neg_p {
            return f32::NAN;
        }
        return signed_inf(neg_p);
    }
    if c.is_infinite() {
        return c;
    }
    if ma == 0 || mb == 0 {
        if mc == 0 {
            return signed_zero(neg_p && neg_c);
        }
        return c;
    }
    if mc == 0 {
        return a * b;
    }

    let (p, kp) = normalise(ma * mb, ka + kb);
    let (cm, kc) = normalise(mc, kc);

    let (x, kx, neg_x, y, ky, neg_y) =
        if kp >= kc { (p, kp, neg_p, cm, kc, neg_c) } else { (cm, kc, neg_c, p, kp, neg_p) };
    let d = (kx - ky) as u32;
    let (ys, sticky) = if d >= 64 { (0, true) } else { (y >> d, y & ((1_u64 << d) - 1) != 0) };
    let y = ys | u64::from(sticky);

    let (s, neg) = if neg_x == neg_y {
        (x + y, neg_x)
    } else if x >= y {
        (x - y, neg_x)
    } else {
        (y - x, neg_y)
    };
    if s == 0 {
        return 0.0;
    }
    round_pack(s, kx, neg)
}
