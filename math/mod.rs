//! Numeric functions: pure code at the item level, not effects (spec.md 7). Each is one portable
//! code on every backend, so `trame::math` is never backend-specific. The module names the type:
//! `math::f32::exp`, `math::f64::exp`.

pub mod f32;
pub mod f64;
