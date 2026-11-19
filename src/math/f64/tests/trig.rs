//! Tests of `trig`: sin, cos, tan, sin_cos functions.

extern crate std;
use std::eprintln;

use super::super::{sin, cos, tan, sin_cos};
use super::{Rng, ulps};

// ============================================================================
// SPECIAL VALUES TEST
// ============================================================================

#[test]
fn sin_special_values() {
    // Basic special cases
    assert_eq!(sin(0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(sin(-0.0).to_bits(), (-0.0_f64).to_bits(), "sin(-0.0) must be -0.0");
    assert!(sin(f64::INFINITY).is_nan(), "sin(+inf) is NaN");
    assert!(sin(f64::NEG_INFINITY).is_nan(), "sin(-inf) is NaN");
    assert!(sin(f64::NAN).is_nan(), "sin(NaN) is NaN");
    
    // Subnormal inputs where sin(x) = x
    let min_sub = f64::from_bits(1);
    assert_eq!(sin(min_sub).to_bits(), min_sub.to_bits());
    
    // pi/4 and nearby values (should be close to std)
    let pi_4 = std::f64::consts::PI / 4.0;
    assert!(ulps(sin(pi_4), pi_4.sin()) <= 2);
    assert!(ulps(sin(-pi_4), (-pi_4).sin()) <= 2);
}

#[test]
fn cos_special_values() {
    // Basic special cases
    assert_eq!(cos(0.0), 1.0);
    assert_eq!(cos(-0.0), 1.0);
    assert!(cos(f64::INFINITY).is_nan(), "cos(+inf) is NaN");
    assert!(cos(f64::NEG_INFINITY).is_nan(), "cos(-inf) is NaN");
    assert!(cos(f64::NAN).is_nan(), "cos(NaN) is NaN");
    
    // Subnormal inputs
    let min_sub = f64::from_bits(1);
    assert_eq!(cos(min_sub), 1.0);
    
    // pi/4 and nearby values
    let pi_4 = std::f64::consts::PI / 4.0;
    assert!(ulps(cos(pi_4), pi_4.cos()) <= 2);
    assert!(ulps(cos(-pi_4), (-pi_4).cos()) <= 2);
}

#[test]
fn tan_special_values() {
    // Basic special cases
    assert_eq!(tan(0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(tan(-0.0).to_bits(), (-0.0_f64).to_bits(), "tan(-0.0) must be -0.0");
    assert!(tan(f64::INFINITY).is_nan(), "tan(+inf) is NaN");
    assert!(tan(f64::NEG_INFINITY).is_nan(), "tan(-inf) is NaN");
    assert!(tan(f64::NAN).is_nan(), "tan(NaN) is NaN");
    
    // Subnormal inputs where tan(x) = x
    let min_sub = f64::from_bits(1);
    assert_eq!(tan(min_sub).to_bits(), min_sub.to_bits());
    
    // pi/4 and nearby values
    let pi_4 = std::f64::consts::PI / 4.0;
    assert!(ulps(tan(pi_4), pi_4.tan()) <= 2);
    assert!(ulps(tan(-pi_4), (-pi_4).tan()) <= 2);
}

#[test]
fn sin_cos_special_values() {
    // Basic special cases
    let (s, c) = sin_cos(0.0);
    assert_eq!(s.to_bits(), 0.0_f64.to_bits());
    assert_eq!(c, 1.0);
    
    let (s, c) = sin_cos(-0.0);
    assert_eq!(s.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(c, 1.0);
    
    // Infinity and NaN
    let (s, c) = sin_cos(f64::INFINITY);
    assert!(s.is_nan() && c.is_nan());
    
    let (s, c) = sin_cos(f64::NEG_INFINITY);
    assert!(s.is_nan() && c.is_nan());
    
    let (s, c) = sin_cos(f64::NAN);
    assert!(s.is_nan() && c.is_nan());
}

// ----------------------------------------------------------------------------------------------
// Error against std (glibc: itself under 1 ulp). One checker, five regimes, three functions. A
// regime is a way of drawing x; the functions are measured on the same draws.

type Draw = fn(&mut Rng) -> f64;

/// `|x| < pi/4`: no reduction.
fn small(r: &mut Rng) -> f64 {
    (r.unit() - 0.5) * core::f64::consts::FRAC_PI_2
}

/// `|x| < 2^20 * pi/2`: the medium reduction of `e_rem_pio2.c`.
fn medium(r: &mut Rng) -> f64 {
    (r.unit() - 0.5) * 2.0 * 1_048_576.0 * core::f64::consts::FRAC_PI_2
}

/// Any finite bit pattern: every exponent, `kernel_rem_pio2` for the large ones, subnormals.
fn any_bits(r: &mut Rng) -> f64 {
    loop {
        let x = f64::from_bits(r.next());
        if x.is_finite() {
            return x;
        }
    }
}

/// The f64 nearest `k * pi/2` for `k` up to 10^6, moved by up to 3 ulps: where the reduced argument
/// is tiny and a reduction that is a few bits short of exact shows as a wrong result.
fn near_multiple(r: &mut Rng) -> f64 {
    let k = 1 + r.next() % 1_000_000;
    let x = k as f64 * core::f64::consts::FRAC_PI_2;
    let step = (r.next() % 7) as i64 - 3;
    f64::from_bits((x.to_bits() as i64 + step) as u64)
}

/// Magnitudes spread over the exponents 2^-60 .. 2^60, random sign: no regime dominates.
fn log_uniform(r: &mut Rng) -> f64 {
    let e = (r.next() % 120) as i32 - 60;
    let m = 1.0 + r.unit();
    let x = m * 2.0_f64.powi(e);
    if r.next() & 1 == 0 { x } else { -x }
}

const REGIMES: [(&str, Draw); 5] =
    [("small", small), ("medium", medium), ("any bits", any_bits), ("near k*pi/2", near_multiple), ("log-uniform", log_uniform)];

fn measure(name: &str, ours: fn(f64) -> f64, theirs: fn(f64) -> f64, seed: u64) {
    for (i, (regime, draw)) in REGIMES.iter().enumerate() {
        let mut rng = Rng(seed + i as u64);
        let (mut worst, mut at) = (0_u64, 0.0_f64);
        for _ in 0..200_000 {
            let x = draw(&mut rng);
            let (a, b) = (ours(x), theirs(x));
            if a.is_nan() || b.is_nan() {
                assert!(a.is_nan() && b.is_nan(), "{name}({x:e}): ours {a:e}, std {b:e}");
                continue;
            }
            let u = ulps(a, b);
            if u > worst {
                (worst, at) = (u, x);
            }
        }
        eprintln!("{name} {regime}: max {worst} ulp against std at x = {at:e}");
        assert!(worst <= 2, "{name} {regime}: {worst} ulp at x = {at:e}");
    }
}

#[test]
fn sin_error_against_std() {
    measure("sin", sin, f64::sin, 0x5eed_f100);
}

#[test]
fn cos_error_against_std() {
    measure("cos", cos, f64::cos, 0x5eed_f200);
}

#[test]
fn tan_error_against_std() {
    measure("tan", tan, f64::tan, 0x5eed_f300);
}

/// A function that ignores the argument reduction (sin of x reduced by a pi/2 that is 1 ulp off)
/// must fail the near-multiple regime, or that regime has no teeth.
#[test]
fn near_multiple_regime_has_teeth() {
    fn crude(x: f64) -> f64 {
        let k = (x / core::f64::consts::FRAC_PI_2 + 0.5) as i64 as f64; // |x| below 2^63
        let r = x - k * core::f64::consts::FRAC_PI_2;
        match (k as i64).rem_euclid(4) {
            0 => sin(r),
            1 => cos(r),
            2 => -sin(r),
            _ => -cos(r),
        }
    }
    let mut rng = Rng(0x5eed_f400);
    let mut worst = 0;
    for _ in 0..20_000 {
        let x = near_multiple(&mut rng);
        worst = worst.max(ulps(crude(x), x.sin()));
    }
    assert!(worst > 1_000, "a one-constant reduction passed the near-multiple regime at {worst} ulp");
}

#[test]
fn sin_cos_is_sin_and_cos_bitwise() {
    let mut rng = Rng(0x5eed_f500);
    for (_, draw) in REGIMES {
        for _ in 0..20_000 {
            let x = draw(&mut rng);
            let (s, c) = sin_cos(x);
            assert_eq!(s.to_bits(), sin(x).to_bits(), "sin_cos({x:e}).0");
            assert_eq!(c.to_bits(), cos(x).to_bits(), "sin_cos({x:e}).1");
        }
    }
}

// ----------------------------------------------------------------------------------------------
// Bit pin. The pin is a change detector, not a correctness claim: it was taken from output that the
// error tests above accept (at most 2 ulp from glibc in five regimes), and it fails when any bit
// moves. If it fails after an intended change, check the error tests first, then update PINNED.

fn fnv(acc: &mut u64, x: f64) {
    for byte in x.to_bits().to_le_bytes() {
        *acc ^= u64::from(byte);
        *acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

#[test]
fn trig_outputs_have_pinned_bits() {
    let mut h = [0xcbf2_9ce4_8422_2325_u64; 3];
    for i in 0..4096_i32 {
        let x = f64::from(i) * 0.732_421_875 - 1500.0;
        fnv(&mut h[0], sin(x));
        fnv(&mut h[1], cos(x));
        fnv(&mut h[2], tan(x));
    }
    let mut rng = Rng(0x5eed_f600);
    for _ in 0..4096 {
        let x = any_bits(&mut rng);
        fnv(&mut h[0], sin(x));
        fnv(&mut h[1], cos(x));
        fnv(&mut h[2], tan(x));
    }
    eprintln!("trig hashes (sin, cos, tan): {h:016x?}");
    assert_eq!(h, PINNED, "trig output bits changed; if intended, update PINNED");
}

/// Order: sin, cos, tan. Pinned on x86_64 with rustc 1.93.1.
const PINNED: [u64; 3] = [0x141d_ef78_32fb_1972, 0xc66e_49c0_87f5_904e, 0x8a15_f5fe_cc9b_a80c];
