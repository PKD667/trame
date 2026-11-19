//! Numeric functions: pure code at the item level, not effects (INTERFACE.md 7). Each is one portable
//! code on every backend, so `trame::math` is never backend-specific. The module names the type:
//! `math::f32::exp`, `math::f64::exp`.

pub mod f32;
pub mod f64;

// The SIMD lane versions of f64 (x86-64 only), behind the lane features (Cargo.toml). Off by default: a build without
// them has no lane code, which is the baseline the lanes are measured against.
#[cfg(all(target_arch = "x86_64", any(feature = "lanes-avx2", feature = "lanes-avx512")))]
pub mod lanes;
