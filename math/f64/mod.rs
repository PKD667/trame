//! The f64 numeric functions of `trame::math` (spec.md 7). Each is one portable code: plain IEEE
//! arithmetic, comparisons and bit manipulation, with no libm, no FMA and no platform transcendental.
//! So the same bits come out on every backend and every target that is IEEE 754 binary64.
//!
//! - `sqrt`, `round`: correctly rounded or exact, by integer arithmetic (see each file).
//! - `exp`, `ln`, `expm1`, `pow`: fdlibm algorithms, error below 1 ulp as fdlibm claims. The
//!   measured errors are in `tests/sweep.rs`.
//! - `powi`: repeated squaring in a fixed order.

// The constants are fdlibm's decimal text, longer than f64 holds and close to std's constants by
// design; math/f64/tests/consts.rs pins each one to its hex word. So the lints are off here.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

mod bits;
mod exp;
mod expm1;
mod ln;
mod pow;
mod powi;
mod round;
mod scalbn;
mod sqrt;

pub use exp::exp;
pub use expm1::exp_m1;
pub use ln::ln;
pub use pow::powf;
pub use powi::powi;
pub use round::round;
pub use sqrt::sqrt;

#[cfg(test)]
mod tests;
