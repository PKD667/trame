//! Round to the nearest integer, halfway cases away from zero, as `f64::round`.
//!
//! Exact on every IEEE target: the code only clears low mantissa bits (the truncation), then tests
//! the dropped part `x - t`. That difference is exact, because `t` and `x` share an exponent and
//! the difference is the dropped bits, which fit in the mantissa. The one addition `t +- 1` adds
//! integers below 2^53, so it is exact too.

use super::bits::{SIGN, copysign};

/// Round `x` to the nearest integer, halfway cases away from zero. Keeps the sign of `x`, so
/// `round(-0.3)` is `-0.0`. NaN, infinities and integers are returned unchanged.
pub fn round(x: f64) -> f64 {
    let bits = x.to_bits();
    let e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    if e >= 52 {
        return x;
    }
    if e < -1 {
        return f64::from_bits(bits & SIGN);
    }
    if e == -1 {
        return copysign(1.0, x);
    }
    let mask = (1_u64 << (52 - e)) - 1;
    let t = f64::from_bits(bits & !mask);
    let frac = (x - t).abs();
    if frac >= 0.5 { t + copysign(1.0, x) } else { t }
}
