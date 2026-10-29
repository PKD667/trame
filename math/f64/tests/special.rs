//! Special values, as IEEE 754 and the fdlibm special cases give them. Where fdlibm departs from
//! IEEE 754 the departure is asserted here and named in the doc comment of the function.

use super::super::{exp, exp_m1, ln, powf, powi, round, sqrt};
use super::ulps;

#[test]
fn exp_special_values() {
    assert_eq!(exp(0.0), 1.0);
    assert_eq!(exp(-0.0), 1.0);
    assert_eq!(exp(f64::INFINITY), f64::INFINITY);
    assert_eq!(exp(f64::NEG_INFINITY), 0.0);
    assert!(exp(f64::NAN).is_nan());
    assert_eq!(exp(710.0), f64::INFINITY);
    assert_eq!(exp(-746.0), 0.0);
    assert_eq!(exp(1e-300), 1.0);
    let sub = exp(-740.0);
    assert!(sub > 0.0 && sub < f64::MIN_POSITIVE, "exp(-740) must be subnormal, got {sub:e}");
    assert!(ulps(sub, (-740.0_f64).exp()) <= 1);
}

#[test]
fn ln_special_values() {
    assert_eq!(ln(1.0), 0.0);
    assert_eq!(ln(0.0), f64::NEG_INFINITY);
    assert_eq!(ln(-0.0), f64::NEG_INFINITY);
    assert!(ln(-1.0).is_nan());
    assert_eq!(ln(f64::INFINITY), f64::INFINITY);
    assert!(ln(f64::NAN).is_nan());
    let min_sub = f64::from_bits(1);
    assert!(ulps(ln(min_sub), min_sub.ln()) <= 1);
}

#[test]
fn expm1_special_values() {
    assert_eq!(exp_m1(0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(exp_m1(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(exp_m1(f64::NEG_INFINITY), -1.0);
    assert_eq!(exp_m1(f64::INFINITY), f64::INFINITY);
    assert!(exp_m1(f64::NAN).is_nan());
    assert_eq!(exp_m1(1e-300), 1e-300);
}

#[test]
fn pow_special_values() {
    assert_eq!(powf(2.0, 10.0), 1024.0);
    assert_eq!(powf(7.0, 0.0), 1.0);
    assert_eq!(powf(f64::NAN, 0.0), 1.0);
    assert_eq!(powf(0.0, -1.0), f64::INFINITY);
    assert_eq!(powf(-0.0, -1.0), f64::NEG_INFINITY);
    assert_eq!(powf(-0.0, -2.0), f64::INFINITY);
    assert!(powf(-8.0, 1.0 / 3.0).is_nan());
    assert_eq!(powf(-2.0, 3.0), -8.0);
    assert_eq!(powf(4.0, 0.5), 2.0);
    assert_eq!(powf(f64::INFINITY, 2.0), f64::INFINITY);
    // Departure from IEEE 754 (fdlibm, kept): powf(1, NaN) and powf(-1, +-inf) are NaN here, where
    // IEEE 754 gives 1. A program that needs IEEE's value must test for it first.
    assert!(powf(1.0, f64::NAN).is_nan());
    assert!(powf(-1.0, f64::INFINITY).is_nan());
}

#[test]
fn sqrt_special_values() {
    assert_eq!(sqrt(0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(sqrt(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(sqrt(f64::INFINITY), f64::INFINITY);
    assert!(sqrt(-1.0).is_nan());
    assert!(sqrt(f64::NEG_INFINITY).is_nan());
    assert!(sqrt(f64::NAN).is_nan());
    assert_eq!(sqrt(f64::from_bits(1)).to_bits(), f64::from_bits(1).sqrt().to_bits());
}

#[test]
fn round_and_powi_exact_cases() {
    assert_eq!(round(-0.3).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(round(0.5), 1.0);
    assert_eq!(round(-0.5), -1.0);
    assert_eq!(round(2.5), 3.0);
    assert_eq!(round(-2.5), -3.0);
    assert_eq!(round(0.49999999999999994), 0.0);
    assert_eq!(round(4503599627370495.5), 4503599627370496.0);
    assert_eq!(round(f64::INFINITY), f64::INFINITY);
    assert_eq!(powi(3.0, 4), 81.0);
    assert_eq!(powi(2.0, -2), 0.25);
    assert_eq!(powi(f64::NAN, 0), 1.0);
    assert_eq!(powi(0.0, -1), f64::INFINITY);
}
