//! SIMD lane versions of `trame::math::f64`, bit-identical to the scalar functions (src/math/f64/).
//! `avx2` (feature `lanes-avx2`, four f64 lanes) and `avx512` (feature `lanes-avx512`, eight). Each
//! function is the scalar's operations per lane, with a guard that sends a vector with special values to
//! the scalar function. The tests (`tests.rs`) compare the bits with the scalar functions.
//!
//! Only on x86-64: the lane code uses `core::arch::x86_64`. Nothing here is called by the library yet; the
//! benchmark (check/math/bench.rs) and the tests are the callers.

#[cfg(feature = "lanes-avx2")]
pub mod avx2;

#[cfg(feature = "lanes-avx512")]
pub mod avx512;

#[cfg(all(test, feature = "lanes-avx2"))]
mod tests;
