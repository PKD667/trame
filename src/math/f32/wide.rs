//! The f32 functions that are the f64 function on the widened argument, rounded once to f32.
//!
//! `f64::from(x)` is exact, the f64 function is one portable code (bit-identical on every target),
//! and `as f32` rounds to nearest even, so each result is one portable code too. The result differs
//! from the correctly rounded f32 value only when the f64 result lies within about 2^-29 relative of
//! a halfway point between two f32 numbers (the f64 error plus the second rounding), so the error
//! stays below 0.5 ulp plus that sliver, i.e. under 1 ulp. For `floor`, `ceil`, `trunc`, `fract`,
//! `round`, `round_ties_even` and `sqrt` the f64 result is exact or correctly rounded and the
//! conversion adds nothing: `sqrt` is correctly rounded for f32 as well (a double rounding through a
//! format with at least 2p+2 bits is innocuous for the square root, here 53 >= 26).
//! `mul_add` is not here: a fused f32 operation through f64 would round twice (see `fma.rs`).

use super::super::f64 as w;

macro_rules! unary {
    ($($name:ident),* $(,)?) => {$(
        #[doc = concat!("`math::f64::", stringify!($name), "` of the widened argument, rounded to f32 (see the module comment).")]
        pub fn $name(x: f32) -> f32 {
            w::$name(f64::from(x)) as f32
        }
    )*};
}

unary!(
    trunc, floor, ceil, fract, round, round_ties_even, sqrt, ln, exp_m1, ln_1p, exp2, log2, log10, cbrt, sin, cos, tan,
    asin, acos, atan, sinh, cosh, tanh, asinh, acosh, atanh,
);

/// `x^y`: `math::f64::powf` of the widened arguments, rounded to f32.
pub fn powf(x: f32, y: f32) -> f32 {
    w::powf(f64::from(x), f64::from(y)) as f32
}

/// `x^n` for an integer `n`: `math::f64::powi` of the widened `x`, rounded to f32. The f64 products
/// are exact for small `n` only in the sense that they round at 53 bits, then once at 24.
pub fn powi(x: f32, n: i32) -> f32 {
    w::powi(f64::from(x), n) as f32
}

/// The angle of the point `(x, y)`, `math::f64::atan2` of the widened arguments, rounded to f32.
pub fn atan2(y: f32, x: f32) -> f32 {
    w::atan2(f64::from(y), f64::from(x)) as f32
}

/// `sqrt(x*x + y*y)` without undue overflow: `math::f64::hypot` of the widened arguments, rounded to f32.
pub fn hypot(x: f32, y: f32) -> f32 {
    w::hypot(f64::from(x), f64::from(y)) as f32
}

/// `(sin x, cos x)`, from the f64 pair rounded to f32.
pub fn sin_cos(x: f32) -> (f32, f32) {
    let (s, c) = w::sin_cos(f64::from(x));
    (s as f32, c as f32)
}
