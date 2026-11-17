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

#[test]
fn max_min_ordinary_values_and_argument_order() {
    use super::super::{max, min};
    assert_eq!(max(1.0, 2.0), 2.0);
    assert_eq!(max(2.0, 1.0), 2.0);
    assert_eq!(min(1.0, 2.0), 1.0);
    assert_eq!(min(2.0, 1.0), 1.0);
    assert_eq!(max(-3.0, -2.0), -2.0);
    assert_eq!(min(-3.0, -2.0), -3.0);
    assert_eq!(max(-2.0, 3.0), 3.0);
    assert_eq!(min(-2.0, 3.0), -2.0);
    assert_eq!(max(5.5, 5.5), 5.5);
    assert_eq!(min(5.5, 5.5), 5.5);
}

#[test]
fn max_min_zero_signs_are_fixed() {
    use super::super::{max, min};
    let pos = 0.0_f64.to_bits();
    let neg = (-0.0_f64).to_bits();
    // max: +0 if either argument is +0, else -0. min: -0 if either argument is -0, else +0.
    for (a, b) in [(0.0_f64, 0.0_f64), (0.0, -0.0), (-0.0, 0.0), (-0.0, -0.0)] {
        let want_max = if a.is_sign_positive() || b.is_sign_positive() { pos } else { neg };
        let want_min = if a.is_sign_negative() || b.is_sign_negative() { neg } else { pos };
        assert_eq!(max(a, b).to_bits(), want_max, "max({a:?}, {b:?})");
        assert_eq!(max(b, a).to_bits(), want_max, "max({b:?}, {a:?})");
        assert_eq!(min(a, b).to_bits(), want_min, "min({a:?}, {b:?})");
        assert_eq!(min(b, a).to_bits(), want_min, "min({b:?}, {a:?})");
    }
    // A zero against a non-zero value: the larger (for max) or smaller (for min) wins, zero's own sign kept.
    assert_eq!(max(-0.0, -1.0).to_bits(), neg);
    assert_eq!(max(-1.0, -0.0).to_bits(), neg);
    assert_eq!(max(0.0, -1.0).to_bits(), pos);
    assert_eq!(min(-0.0, 1.0).to_bits(), neg);
    assert_eq!(min(1.0, -0.0).to_bits(), neg);
    assert_eq!(min(0.0, 1.0).to_bits(), pos);
}

#[test]
fn max_min_nan_in_either_position() {
    use super::super::{max, min};
    for nan in [f64::NAN, -f64::NAN] {
        assert_eq!(max(nan, 1.0), 1.0);
        assert_eq!(max(1.0, nan), 1.0);
        assert_eq!(min(nan, -1.0), -1.0);
        assert_eq!(min(-1.0, nan), -1.0);
        assert_eq!(max(nan, -0.0).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(min(0.0, nan).to_bits(), 0.0_f64.to_bits());
        assert!(max(nan, f64::NAN).is_nan());
        assert!(min(nan, f64::NAN).is_nan());
        assert!(max(f64::NAN, nan).is_nan());
        assert!(min(f64::NAN, nan).is_nan());
    }
}

#[test]
fn max_min_infinities() {
    use super::super::{max, min};
    assert_eq!(max(f64::INFINITY, 1.0), f64::INFINITY);
    assert_eq!(max(1.0, f64::INFINITY), f64::INFINITY);
    assert_eq!(max(f64::NEG_INFINITY, 1.0), 1.0);
    assert_eq!(max(f64::INFINITY, f64::NEG_INFINITY), f64::INFINITY);
    assert_eq!(max(f64::NEG_INFINITY, f64::NEG_INFINITY), f64::NEG_INFINITY);
    assert_eq!(min(f64::NEG_INFINITY, 1.0), f64::NEG_INFINITY);
    assert_eq!(min(1.0, f64::NEG_INFINITY), f64::NEG_INFINITY);
    assert_eq!(min(f64::INFINITY, 1.0), 1.0);
    assert_eq!(min(f64::INFINITY, f64::NEG_INFINITY), f64::NEG_INFINITY);
    assert_eq!(min(f64::INFINITY, f64::INFINITY), f64::INFINITY);
}

#[test]
fn max_min_agree_with_core_off_zero() {
    // Core's max and min are unspecified only for a zero result, so off zero they must match bit for bit.
    // Pairs with a NaN are excluded here: core's NaN payload is unspecified too (the checked cases above cover them).
    use super::super::{max, min};
    let mut rng = super::Rng(0x5eed_0002);
    let mut checked = 0_u32;
    for _ in 0..200_000 {
        let a = f64::from_bits(rng.next());
        let b = f64::from_bits(rng.next());
        if a.is_nan() || b.is_nan() {
            continue;
        }
        if a == 0.0 && b == 0.0 {
            continue;
        }
        assert_eq!(max(a, b).to_bits(), a.max(b).to_bits(), "max({a:e}, {b:e})");
        assert_eq!(min(a, b).to_bits(), a.min(b).to_bits(), "min({a:e}, {b:e})");
        checked += 1;
    }
    assert!(checked > 190_000, "only {checked} pairs checked");
}
