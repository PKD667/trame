//! Square root, correctly rounded (IEEE 754), in integer arithmetic only.
//!
//! Argument. Write x = m * 2^e with m an integer in [2^52, 2^54) and e even (normalised and made
//! even by shifting m). Then sqrt(x) = sqrt(m * 2^60) * 2^((e-60)/2). The integer square root r of
//! n = m * 2^60 is taken by the digit method, which gives floor(sqrt n), and `r * r == n` says
//! whether the root is exact. r lies in [2^56, 2^57), so 4 low bits are dropped to get 53 bits;
//! the rounding is nearest, ties to even, where a tie is inexact only when the remainder is zero.
//! The scaling by a power of two is exact because the result is normal. So the result is the
//! correctly rounded root, on every target, without relying on the platform's sqrt.

/// Correctly rounded square root. `sqrt(-0.0)` is `-0.0`; negative inputs give NaN; NaN is returned
/// as is (its payload is unspecified by IEEE 754, see spec.md, Requirement (Floating point)).
pub fn sqrt(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 || x == f64::INFINITY {
        return x;
    }
    let bits = x.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    let (mut m, mut e) = if biased == 0 { (frac, -1074_i64) } else { (frac | (1 << 52), biased - 1075) };
    while m < (1 << 52) {
        m <<= 1;
        e -= 1;
    }
    if (e & 1) != 0 {
        m <<= 1;
        e -= 1;
    }
    let n = u128::from(m) << 60;
    let r = isqrt(n);
    let exact = r * r == n;
    let half_e = (e - 60) / 2;
    // r is in [2^56, 2^57): drop k = 4 bits to keep 53.
    let k = 4_u32;
    let mut q = (r >> k) as u64;
    let rem = r & ((1_u128 << k) - 1);
    let half = 1_u128 << (k - 1);
    if rem > half || (rem == half && (!exact || (q & 1) == 1)) {
        q += 1;
    }
    let mut e2 = half_e + i64::from(k);
    if q == 1 << 53 {
        q >>= 1;
        e2 += 1;
    }
    (q as f64) * pow2(e2)
}

/// `floor(sqrt(n))` by the digit method: one bit pair per step, integer operations only.
fn isqrt(n: u128) -> u128 {
    let mut rem = n;
    let mut root = 0_u128;
    let mut bit = 1_u128 << 126;
    while bit > rem {
        bit >>= 2;
    }
    while bit != 0 {
        if rem >= root + bit {
            rem -= root + bit;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root
}

/// `2^e` for `e` in the normal range, from its bits.
fn pow2(e: i64) -> f64 {
    f64::from_bits(((e + 1023) as u64) << 52)
}
