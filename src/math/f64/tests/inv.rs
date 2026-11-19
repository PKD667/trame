//! Tests of inverse and hyperbolic trig functions.

extern crate std;
use std::eprintln;

use super::super::{acos, acosh, asin, asinh, atan, atan2, atanh, cosh, ln_1p, sinh, tanh};
use super::{Rng, ulps};

// ===== Special cases =====

#[test]
fn atan_special_values() {
    assert_eq!(atan(0.0), 0.0);
    assert_eq!(atan(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(atan(f64::INFINITY), std::f64::consts::FRAC_PI_2);
    assert_eq!(atan(f64::NEG_INFINITY), -std::f64::consts::FRAC_PI_2);
    assert!(atan(f64::NAN).is_nan());

    // Test some known values
    let eps = 1e-14;
    let result = atan(1.0);
    let expected = std::f64::consts::FRAC_PI_4;
    assert!((result - expected).abs() < eps);
}

#[test]
fn atan2_special_values() {
    // y = 0, x > 0
    assert_eq!(atan2(0.0, 1.0), 0.0);
    assert_eq!(atan2(-0.0, 1.0).to_bits(), (-0.0_f64).to_bits());

    // y > 0, x = 0
    assert!(atan2(1.0, 0.0) > std::f64::consts::FRAC_PI_2 - 1e-14);
    assert!(atan2(1.0, 0.0) < std::f64::consts::FRAC_PI_2 + 1e-14);

    // y > 0, x < 0
    let result = atan2(1.0, -1.0);
    let expected = 3.0 * std::f64::consts::FRAC_PI_4;
    assert!((result - expected).abs() < 1e-14);

    // atan2(0, -anything) should be +/- pi
    let result = atan2(0.0, -1.0);
    assert!((result - std::f64::consts::PI).abs() < 1e-14 || (result + std::f64::consts::PI).abs() < 1e-14);

    // infinities (C99 F.9.1.4): atan2(+-inf, +inf) = +-pi/4, atan2(+-inf, -inf) = +-3pi/4
    assert_eq!(atan2(f64::INFINITY, f64::INFINITY).to_bits(), 0x3fe9_21fb_5444_2d18);
    assert_eq!(atan2(f64::INFINITY, f64::NEG_INFINITY).to_bits(), 0x4002_d97c_7f33_21d2);
    assert_eq!(atan2(f64::NEG_INFINITY, f64::INFINITY).to_bits(), 0xbfe9_21fb_5444_2d18);
    assert_eq!(atan2(f64::NEG_INFINITY, f64::NEG_INFINITY).to_bits(), 0xc002_d97c_7f33_21d2);
    assert!(atan2(f64::NAN, 1.0).is_nan());
    assert!(atan2(1.0, f64::NAN).is_nan());
}

#[test]
fn asin_special_values() {
    assert_eq!(asin(0.0), 0.0);
    assert_eq!(asin(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert!(asin(2.0).is_nan());
    assert!(asin(-2.0).is_nan());
    assert!(asin(f64::NAN).is_nan());

    // asin(1) = pi/2
    let result = asin(1.0);
    let expected = std::f64::consts::FRAC_PI_2;
    assert!(ulps(result, expected) <= 1);

    // asin(-1) = -pi/2
    let result = asin(-1.0);
    let expected = -std::f64::consts::FRAC_PI_2;
    assert!(ulps(result, expected) <= 1);
}

#[test]
fn acos_special_values() {
    // acos(1) = 0
    assert_eq!(acos(1.0), 0.0);

    // acos(-1) = pi
    let result = acos(-1.0);
    let expected = std::f64::consts::PI;
    assert!(ulps(result, expected) <= 2);

    // acos(0) = pi/2
    let result = acos(0.0);
    let expected = std::f64::consts::FRAC_PI_2;
    assert!(ulps(result, expected) <= 1);

    // Domain errors
    assert!(acos(2.0).is_nan());
    assert!(acos(-2.0).is_nan());
    assert!(acos(f64::NAN).is_nan());
}

#[test]
fn sinh_special_values() {
    assert_eq!(sinh(0.0), 0.0);
    assert_eq!(sinh(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(sinh(f64::INFINITY), f64::INFINITY);
    assert_eq!(sinh(f64::NEG_INFINITY), f64::NEG_INFINITY);
    assert!(sinh(f64::NAN).is_nan());
}

#[test]
fn cosh_special_values() {
    assert_eq!(cosh(0.0), 1.0);
    assert_eq!(cosh(-0.0), 1.0);
    assert_eq!(cosh(f64::INFINITY), f64::INFINITY);
    assert_eq!(cosh(f64::NEG_INFINITY), f64::INFINITY);
    assert!(cosh(f64::NAN).is_nan());
}

#[test]
fn tanh_special_values() {
    assert_eq!(tanh(0.0), 0.0);
    assert_eq!(tanh(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(tanh(f64::INFINITY), 1.0);
    assert_eq!(tanh(f64::NEG_INFINITY), -1.0);
    assert!(tanh(f64::NAN).is_nan());
}

#[test]
fn asinh_special_values() {
    assert_eq!(asinh(0.0), 0.0);
    assert_eq!(asinh(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(asinh(f64::INFINITY), f64::INFINITY);
    assert_eq!(asinh(f64::NEG_INFINITY), f64::NEG_INFINITY);
    assert!(asinh(f64::NAN).is_nan());
}

#[test]
fn acosh_special_values() {
    // acosh(1) = 0
    assert_eq!(acosh(1.0), 0.0);

    // acosh(0.5) should be NaN (domain error)
    assert!(acosh(0.5).is_nan());

    // acosh(-1) should be NaN
    assert!(acosh(-1.0).is_nan());

    // acosh(inf) = inf
    assert_eq!(acosh(f64::INFINITY), f64::INFINITY);

    // acosh(NaN) = NaN
    assert!(acosh(f64::NAN).is_nan());
}

#[test]
fn atanh_special_values() {
    assert_eq!(atanh(0.0), 0.0);
    assert_eq!(atanh(-0.0).to_bits(), (-0.0_f64).to_bits());

    // atanh(1) = inf
    assert_eq!(atanh(1.0), f64::INFINITY);

    // atanh(-1) = -inf
    assert_eq!(atanh(-1.0), f64::NEG_INFINITY);

    // atanh(2) = NaN (outside domain)
    assert!(atanh(2.0).is_nan());

    // atanh(NaN) = NaN
    assert!(atanh(f64::NAN).is_nan());
}

#[test]
fn ln_1p_special_values() {
    // ln_1p(0) = 0
    assert_eq!(ln_1p(0.0), 0.0);
    assert_eq!(ln_1p(-0.0).to_bits(), (-0.0_f64).to_bits());

    // ln_1p(-1) = -inf
    assert_eq!(ln_1p(-1.0), f64::NEG_INFINITY);

    // ln_1p(-2) = NaN
    assert!(ln_1p(-2.0).is_nan());

    // ln_1p(inf) = inf
    assert_eq!(ln_1p(f64::INFINITY), f64::INFINITY);

    // ln_1p(NaN) = NaN
    assert!(ln_1p(f64::NAN).is_nan());

    // ln_1p(1) = ln(2)
    let result = ln_1p(1.0);
    let expected = 2.0_f64.ln();
    assert!(ulps(result, expected) <= 1);
}

// ===== Error bounds against std =====

const N: usize = 200_000;

fn max_ulps_inv(name: &str, mut f: impl FnMut(&mut Rng) -> (f64, f64)) -> u64 {
    let mut rng = Rng(0x1ee7_0001);
    let mut worst = 0;
    for _ in 0..N {
        let (ours, theirs) = f(&mut rng);
        if ours.is_nan() || theirs.is_nan() {
            if ours.is_nan() && theirs.is_nan() {
                continue;
            }
            // NaN mismatch
            panic!("{name}: NaN mismatch: ours={ours:e}, theirs={theirs:e}");
        }
        worst = worst.max(ulps(ours, theirs));
    }
    eprintln!("{name}: max {worst} ulp against std over {N} inputs");
    worst
}

#[test]
fn atan_error_against_std() {
    let worst = max_ulps_inv("atan", |r| {
        let x = -100.0 + 200.0 * r.unit();
        (atan(x), x.atan())
    });
    assert!(worst <= 2, "atan: {worst} ulp");
}

#[test]
fn atan2_error_against_std() {
    let worst = max_ulps_inv("atan2", |r| {
        let y = -100.0 + 200.0 * r.unit();
        let x = -100.0 + 200.0 * r.unit();
        (atan2(y, x), y.atan2(x))
    });
    assert!(worst <= 2, "atan2: {worst} ulp");
}

#[test]
fn asin_error_against_std() {
    let worst = max_ulps_inv("asin", |r| {
        let x = -1.0 + 2.0 * r.unit();
        let x = x.clamp(-1.0, 1.0); // Ensure in domain
        (asin(x), x.asin())
    });
    assert!(worst <= 2, "asin: {worst} ulp");
}

#[test]
fn acos_error_against_std() {
    let worst = max_ulps_inv("acos", |r| {
        let x = -1.0 + 2.0 * r.unit();
        let x = x.clamp(-1.0, 1.0); // Ensure in domain
        (acos(x), x.acos())
    });
    assert!(worst <= 2, "acos: {worst} ulp");
}

#[test]
fn sinh_error_against_std() {
    let worst = max_ulps_inv("sinh", |r| {
        let x = -40.0 + 80.0 * r.unit();
        (sinh(x), x.sinh())
    });
    assert!(worst <= 2, "sinh: {worst} ulp");
}

#[test]
fn cosh_error_against_std() {
    let worst = max_ulps_inv("cosh", |r| {
        let x = -40.0 + 80.0 * r.unit();
        (cosh(x), x.cosh())
    });
    assert!(worst <= 2, "cosh: {worst} ulp");
}

#[test]
fn tanh_error_against_std() {
    let worst = max_ulps_inv("tanh", |r| {
        let x = -20.0 + 40.0 * r.unit();
        (tanh(x), x.tanh())
    });
    assert!(worst <= 2, "tanh: {worst} ulp");
}

#[test]
fn asinh_error_against_std() {
    let worst = max_ulps_inv("asinh", |r| {
        let x = -1000.0 + 2000.0 * r.unit();
        (asinh(x), x.asinh())
    });
    assert!(worst <= 2, "asinh: {worst} ulp");
}

#[test]
fn acosh_error_against_std() {
    let worst = max_ulps_inv("acosh", |r| {
        let x = 1.0 + 1000.0 * r.unit(); // acosh domain is [1, inf)
        (acosh(x), x.acosh())
    });
    assert!(worst <= 2, "acosh: {worst} ulp");
}

// atanh is judged against correctly rounded values in tests/vectors.rs. There is no `atanh_error_against_std`: std's
// atanh is the formula 0.5 * ln_1p(2x / (1 - x)), which is 4 ulp off even on [-0.5, 0.5] and ~1000 ulp near -1, so a
// comparison with it measures std, not us.

#[test]
fn ln_1p_error_against_std() {
    let worst = max_ulps_inv("ln_1p", |r| {
        let x = -0.999 + 100.0 * r.unit();
        let x = if x <= -1.0 { -0.9999 } else { x }; // Avoid -1.0 and below
        (ln_1p(x), x.ln_1p())
    });
    assert!(worst <= 2, "ln_1p: {worst} ulp");
}

// ===== Pinned hash test =====

fn fnv(acc: &mut u64, x: f64) {
    for byte in x.to_bits().to_le_bytes() {
        *acc ^= u64::from(byte);
        *acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn inv_grid() -> impl Iterator<Item = f64> {
    (0..4096_i32).map(|i| f64::from(i) * 0.732_421_875 - 1500.0)
}

#[test]
fn inv_outputs_have_pinned_bits() {
    let mut h = [0xcbf2_9ce4_8422_2325_u64; 11];
    for x in inv_grid() {
        // Clamp values to valid domains for each function
        let x_atan = x;
        let x_asin = (x * 0.0001).clamp(-1.0, 1.0);
        let x_acos = (x * 0.0001).clamp(-1.0, 1.0);
        let x_sinh = x * 0.01;
        let x_acosh = (x * 0.001).max(1.0);
        let x_atanh = (x * 0.00001).clamp(-0.9999, 0.9999);
        let x_ln1p = (x * 0.0001).clamp(-0.9999, 100.0);

        fnv(&mut h[0], atan(x_atan));
        fnv(&mut h[1], atan2(x * 0.5, x.abs() + 1.0));
        fnv(&mut h[2], asin(x_asin));
        fnv(&mut h[3], acos(x_acos));
        fnv(&mut h[4], sinh(x_sinh));
        fnv(&mut h[5], cosh(x_sinh));
        fnv(&mut h[6], tanh(x_sinh));
        fnv(&mut h[7], asinh(x * 0.1));
        fnv(&mut h[8], acosh(x_acosh));
        fnv(&mut h[9], atanh(x_atanh));
        fnv(&mut h[10], ln_1p(x_ln1p));
    }
    eprintln!("inv hashes: {h:016x?}");
    assert_eq!(h, PINNED, "output bits changed; if intended, update PINNED");
}

/// Pinned on x86_64 with rustc 1.93.1. The spec says every IEEE 754 target gives these bits;
/// no other target has run this.
const PINNED: [u64; 11] = [
    0xf9957cd33e1c5c27, // atan
    0xe7b066e5a387bffb, // atan2
    0x6293be3642df826c, // asin
    0x47ec59f3a818c598, // acos
    0xc6ff8bc3b028025d, // sinh
    0x8d320e5a83264b1c, // cosh
    0xc1df7b0860118fc6, // tanh
    0x17c0574189afdbcc, // asinh
    0x8a583931a2b14cab, // acosh
    0xdb7d448993283e12, // atanh
    0x321dc51337f0f373, // ln_1p
];
