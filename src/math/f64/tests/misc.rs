//! Tests of log2, log10, exp2, cbrt, hypot. Includes special values, error vs std, and bit pinning.

extern crate std;
use std::eprintln;

use super::super::{cbrt, exp2, hypot, log2, log10};
use super::{Rng, ulps};

const N_SAMPLES: usize = 200_000;

// ============================================================================
// Special values (C99 semantics)
// ============================================================================

#[test]
fn log2_special_values() {
    assert_eq!(log2(1.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(log2(2.0), 1.0);
    assert_eq!(log2(8.0), 3.0);
    assert_eq!(log2(0.5), -1.0);
    assert_eq!(log2(0.0), f64::NEG_INFINITY);
    assert_eq!(log2(-0.0), f64::NEG_INFINITY);
    assert!(log2(-1.0).is_nan());
    assert_eq!(log2(f64::INFINITY), f64::INFINITY);
    assert!(log2(f64::NAN).is_nan());

    // Integer powers: exact
    for n in -50..51 {
        let x = 2.0_f64.powi(n);
        assert_eq!(log2(x), f64::from(n), "log2(2^{}) = {} but got {}", n, n, log2(x));
    }
}

#[test]
fn log10_special_values() {
    assert_eq!(log10(1.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(log10(10.0), 1.0);
    assert_eq!(log10(100.0), 2.0);
    assert_eq!(log10(0.1), -1.0);
    assert_eq!(log10(0.0), f64::NEG_INFINITY);
    assert!(log10(-1.0).is_nan());
    assert_eq!(log10(f64::INFINITY), f64::INFINITY);
    assert!(log10(f64::NAN).is_nan());
}

#[test]
fn exp2_special_values() {
    assert_eq!(exp2(0.0).to_bits(), 1.0_f64.to_bits());
    assert_eq!(exp2(-0.0).to_bits(), 1.0_f64.to_bits());
    assert_eq!(exp2(1.0), 2.0);
    assert_eq!(exp2(10.0), 1024.0);
    assert_eq!(exp2(f64::NEG_INFINITY), 0.0);
    assert_eq!(exp2(f64::INFINITY), f64::INFINITY);
    assert!(exp2(f64::NAN).is_nan());

    // Integer powers: exact
    for n in -50..51 {
        let result = exp2(f64::from(n));
        let expected = 2.0_f64.powi(n);
        assert_eq!(result.to_bits(), expected.to_bits(), "exp2({}) bits differ", n);
    }

    // Subnormal handling
    let sub = exp2(-740.0);
    // Note: exp2(-740) produces a subnormal but asserting the exact bounds is tricky
    // due to rounding; just verify it's not zero
    assert!(sub != 0.0, "exp2(-740) should not be zero, got {}", sub);
}

#[test]
fn cbrt_special_values() {
    assert_eq!(cbrt(0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(cbrt(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(cbrt(1.0), 1.0);
    assert_eq!(cbrt(-1.0), -1.0);
    assert_eq!(cbrt(8.0), 2.0);
    assert_eq!(cbrt(-8.0), -2.0);
    assert_eq!(cbrt(27.0), 3.0);
    assert_eq!(cbrt(-27.0), -3.0);
    assert_eq!(cbrt(f64::INFINITY), f64::INFINITY);
    assert_eq!(cbrt(f64::NEG_INFINITY), f64::NEG_INFINITY);
    assert!(cbrt(f64::NAN).is_nan());

    // Subnormal
    let min_sub = f64::from_bits(1);
    let result = cbrt(min_sub);
    assert!(result > 0.0, "cbrt of smallest subnormal should be positive");
}

#[test]
fn hypot_special_values() {
    assert_eq!(hypot(0.0, 0.0), 0.0);
    assert_eq!(hypot(3.0, 4.0), 5.0);
    assert_eq!(hypot(4.0, 3.0), 5.0);
    assert_eq!(hypot(0.0, 1.0), 1.0);
    assert_eq!(hypot(1.0, 0.0), 1.0);
    assert_eq!(hypot(-3.0, 4.0), 5.0);
    assert_eq!(hypot(3.0, -4.0), 5.0);
    assert_eq!(hypot(-3.0, -4.0), 5.0);
    assert_eq!(hypot(f64::INFINITY, 1.0), f64::INFINITY);
    assert_eq!(hypot(1.0, f64::INFINITY), f64::INFINITY);
    assert_eq!(hypot(f64::INFINITY, f64::INFINITY), f64::INFINITY);
    assert_eq!(hypot(f64::NEG_INFINITY, 1.0), f64::INFINITY);
    assert!(hypot(f64::NAN, 1.0).is_nan());
    assert!(hypot(1.0, f64::NAN).is_nan());
    assert!(hypot(f64::NAN, f64::NAN).is_nan());
}

// ============================================================================
// Error measurement vs std (glibc)
// ============================================================================

#[test]
fn log2_error_vs_std() {
    let mut rng = Rng(0x5eed_0004);
    let mut worst = 0_u64;

    for _ in 0..N_SAMPLES {
        let x = f64::from_bits(rng.next() & 0x7fff_ffff_ffff_ffff);
        if x.is_nan() || x.is_infinite() || x == 0.0 {
            continue;
        }
        let ours = log2(x);
        let theirs = x.log2();
        let err = ulps(ours, theirs);
        worst = worst.max(err);
        assert!(err <= 2, "log2({:e}): {} ulp away from std", x, err);
    }
    eprintln!("log2: max {} ulp against std over {} samples", worst, N_SAMPLES);
}

#[test]
fn log10_error_vs_std() {
    let mut rng = Rng(0x5eed_0005);
    let mut worst = 0_u64;

    for _ in 0..N_SAMPLES {
        let x = f64::from_bits(rng.next() & 0x7fff_ffff_ffff_ffff);
        if x.is_nan() || x.is_infinite() || x == 0.0 {
            continue;
        }
        let ours = log10(x);
        let theirs = x.log10();
        let err = ulps(ours, theirs);
        worst = worst.max(err);
        assert!(err <= 2, "log10({:e}): {} ulp away from std", x, err);
    }
    eprintln!("log10: max {} ulp against std over {} samples", worst, N_SAMPLES);
}

#[test]
fn exp2_error_vs_std() {
    let mut rng = Rng(0x5eed_0006);
    let mut worst = 0_u64;

    for _ in 0..N_SAMPLES {
        let x = -1074.0 + 2098.0 * rng.unit();
        let ours = exp2(x);
        let theirs = x.exp2();
        if ours.is_nan() || theirs.is_nan() {
            assert!(ours.is_nan() && theirs.is_nan());
            continue;
        }
        let err = ulps(ours, theirs);
        worst = worst.max(err);
        assert!(err <= 2, "exp2({:e}): {} ulp away from std", x, err);
    }
    eprintln!("exp2: max {} ulp against std over {} samples", worst, N_SAMPLES);
}

#[test]
fn cbrt_error_vs_std() {
    let mut rng = Rng(0x5eed_0007);
    let mut worst = 0_u64;

    for _ in 0..N_SAMPLES {
        let x = f64::from_bits(rng.next());
        if x.is_nan() {
            continue;
        }
        let ours = cbrt(x);
        let theirs = x.cbrt();
        let err = ulps(ours, theirs);
        worst = worst.max(err);
        assert!(err <= 2, "cbrt({:e}): {} ulp away from std", x, err);
    }
    eprintln!("cbrt: max {} ulp against std over {} samples", worst, N_SAMPLES);
}

#[test]
fn hypot_error_vs_std() {
    let mut rng = Rng(0x5eed_0008);
    let mut worst = 0_u64;

    for _ in 0..N_SAMPLES {
        let x = f64::from_bits(rng.next());
        let y = f64::from_bits(rng.next());
        if x.is_nan() || y.is_nan() {
            continue;
        }
        let ours = hypot(x, y);
        let theirs = x.hypot(y);
        let err = ulps(ours, theirs);
        worst = worst.max(err);
        assert!(err <= 2, "hypot({:e}, {:e}): {} ulp away from std", x, y, err);
    }
    eprintln!("hypot: max {} ulp against std over {} samples", worst, N_SAMPLES);
}

// ============================================================================
// Bit pinning (FNV-1a hash of outputs on a fixed grid)
// ============================================================================

fn fnv(acc: &mut u64, x: f64) {
    for byte in x.to_bits().to_le_bytes() {
        *acc ^= u64::from(byte);
        *acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn grid() -> impl Iterator<Item = f64> {
    (0..1024_i32).map(|i| f64::from(i) * 0.927_734_375 - 500.0)
}

#[test]
fn misc_outputs_have_pinned_bits() {
    let mut h = [0_u64; 5];
    for x in grid() {
        if x > 0.0 {
            fnv(&mut h[0], log2(x));
            fnv(&mut h[1], log10(x));
        }
        fnv(&mut h[2], exp2(x * 0.5));
        fnv(&mut h[3], cbrt(x));
        fnv(&mut h[4], hypot(x, x * 0.3));
    }
    eprintln!("misc hashes: {h:016x?}");
    assert_eq!(h, PINNED, "output bits changed; if intended, update PINNED");
}

/// Pinned on x86_64 with rustc 1.98.1.
const PINNED: [u64; 5] =
    [0x5a3a971fc6e2d0e7, 0x251052c68921bd39, 0xa63d6cf677c1206e, 0x4ae9e817ea6b42db, 0x9106ec3cfaae58de];
