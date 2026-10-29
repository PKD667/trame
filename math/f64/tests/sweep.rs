//! Error of each function against std over a fixed pseudo-random sweep. std is the platform libm
//! (glibc here), which is itself within about 1 ulp, so a measured distance of 2 ulp can be the
//! other side's error. The bounds are measured, not proved; see the printed maxima.
//! Run with `--nocapture` to see them.

extern crate std;
use std::eprintln;

use super::super::{exp, exp_m1, ln, powf, round, sqrt};
use super::{Rng, ulps};

const N: usize = 200_000;

fn max_ulps(name: &str, mut f: impl FnMut(&mut Rng) -> (f64, f64)) -> u64 {
    let mut rng = Rng(0x5eed_0001);
    let mut worst = 0;
    for _ in 0..N {
        let (ours, theirs) = f(&mut rng);
        if ours.is_nan() || theirs.is_nan() {
            assert!(ours.is_nan() && theirs.is_nan(), "{name}: NaN mismatch");
            continue;
        }
        worst = worst.max(ulps(ours, theirs));
    }
    eprintln!("{name}: max {worst} ulp against std over {N} inputs");
    worst
}

#[test]
fn exp_error_against_std() {
    let worst = max_ulps("exp", |r| {
        let x = -745.0 + 1455.0 * r.unit();
        (exp(x), x.exp())
    });
    assert!(worst <= 2, "exp: {worst} ulp");
}

#[test]
fn ln_error_against_std() {
    let worst = max_ulps("ln", |r| {
        // Random bit patterns: covers subnormals and every exponent.
        let x = f64::from_bits(r.next() & 0x7fff_ffff_ffff_ffff);
        if x.is_nan() || x.is_infinite() || x == 0.0 {
            return (0.0, 0.0);
        }
        (ln(x), x.ln())
    });
    assert!(worst <= 2, "ln: {worst} ulp");
}

#[test]
fn expm1_error_against_std() {
    let worst = max_ulps("expm1", |r| {
        let x = -40.0 + 80.0 * r.unit();
        (exp_m1(x), x.exp_m1())
    });
    assert!(worst <= 2, "expm1: {worst} ulp");
}

#[test]
fn pow_error_against_std() {
    let worst = max_ulps("pow", |r| {
        let x = 10.0 * r.unit();
        let y = -20.0 + 40.0 * r.unit();
        (powf(x, y), x.powf(y))
    });
    assert!(worst <= 2, "pow: {worst} ulp");
}

#[test]
fn sqrt_is_bitwise_std() {
    let mut rng = Rng(0x5eed_0002);
    for _ in 0..N {
        let x = f64::from_bits(rng.next() & 0x7fff_ffff_ffff_ffff);
        if x.is_nan() {
            continue;
        }
        assert_eq!(sqrt(x).to_bits(), x.sqrt().to_bits(), "sqrt({x:e})");
    }
}

#[test]
fn round_is_bitwise_std() {
    let mut rng = Rng(0x5eed_0003);
    for _ in 0..N {
        let x = f64::from_bits(rng.next());
        if x.is_nan() {
            continue;
        }
        assert_eq!(round(x).to_bits(), x.round().to_bits(), "round({x:e})");
    }
}
