//! `trunc`, `floor`, `ceil`, `fract` and `round_ties_even` of f64: exact on every IEEE target.
//!
//! Each clears the low mantissa bits of `x` (the truncation `t`, exact by construction), then adds or
//! subtracts 1.0 to an integer below 2^53 when the dropped part asks for it. The one other operation
//! is `x - t`, exact because `t` and `x` share an exponent. The results equal std's bit for bit,
//! signed zeros included (`floor(-0.0)` is `-0.0`, `ceil(-0.3)` is `-0.0`).

use super::bits::{SIGN, copysign};

/// Biased-exponent-free exponent of `x`: `floor(log2 |x|)` for normal `x`, -1023 for zero and subnormals.
fn exponent(x: f64) -> i64 {
    ((x.to_bits() >> 52) & 0x7ff) as i64 - 1023
}

/// `x` rounded toward zero. NaN, infinities and integers are returned unchanged; `trunc(-0.3)` is `-0.0`.
pub fn trunc(x: f64) -> f64 {
    let e = exponent(x);
    if e >= 52 {
        return x;
    }
    if e < 0 {
        return f64::from_bits(x.to_bits() & SIGN);
    }
    let mask = (1_u64 << (52 - e)) - 1;
    f64::from_bits(x.to_bits() & !mask)
}

/// The largest integer not above `x`. NaN, infinities and integers are returned unchanged;
/// `floor(-0.0)` is `-0.0`, and `floor(-0.3)` is `-1.0`.
pub fn floor(x: f64) -> f64 {
    let t = trunc(x);
    if x < t { t - 1.0 } else { t }
}

/// The smallest integer not below `x`. NaN, infinities and integers are returned unchanged;
/// `ceil(-0.3)` is `-0.0`, and `ceil(0.3)` is `1.0`.
pub fn ceil(x: f64) -> f64 {
    let t = trunc(x);
    if x > t { t + 1.0 } else { t }
}

/// The fractional part, `x - trunc(x)`, with the sign of `x`. NaN for an infinity, as std.
pub fn fract(x: f64) -> f64 {
    x - trunc(x)
}

/// Round to the nearest integer, halfway cases to the even neighbour (IEEE `roundToIntegralTiesToEven`).
/// Adding and subtracting 2^52 rounds a value of size below 2^52 to an integer under round to nearest
/// even, in plain arithmetic. NaN, infinities and integers are returned unchanged.
pub fn round_ties_even(x: f64) -> f64 {
    const TWO52: f64 = 4_503_599_627_370_496.0;
    let a = x.abs();
    if a.is_nan() || a >= TWO52 {
        return x;
    }
    copysign((a + TWO52) - TWO52, x)
}
