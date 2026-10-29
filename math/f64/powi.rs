//! Integer power by repeated squaring, in a fixed order, so every backend gives the same bits.
//!
//! The order: the bits of `|n|` are read from the least significant; for each set bit the
//! accumulator is multiplied by the current square of `x`. Each product rounds once (IEEE, round
//! to nearest even, no FMA), so the result is determined by `x` and `n` alone. The error is about
//! one rounding per multiplication, so it grows with `log2 |n|`; there is no correctly-rounded claim.

/// `x` to the integer power `n`: `x^n` for `n >= 0`, `1 / x^|n|` for `n < 0`. `powi(x, 0)` is 1.
pub fn powi(x: f64, n: i32) -> f64 {
    let mut base = x;
    let mut e = n.unsigned_abs();
    let mut acc = 1.0;
    while e > 0 {
        if e & 1 == 1 {
            acc *= base;
        }
        e >>= 1;
        if e > 0 {
            base *= base;
        }
    }
    if n < 0 { 1.0 / acc } else { acc }
}
