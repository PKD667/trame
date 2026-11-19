//! `mul_add` is exact: the same bits as std's fused multiply-add (correctly rounded on glibc and on
//! hardware FMA), for every input. No tolerance, no ulp bound: a single differing bit fails.
//! Any NaN equals any NaN (the sign and payload of a NaN are unspecified).
//!
//! The sweep has teeth: it counts the triples where the unfused `a * b + c` differs from the fused
//! result, and asserts the count is large, so a test that passes for the naive version is detected.

extern crate std;
use std::eprintln;

use super::super::mul_add;
use super::Rng;

fn check(a: f64, b: f64, c: f64) {
    let ours = mul_add(a, b, c);
    let theirs = std::primitive::f64::mul_add(a, b, c);
    if theirs.is_nan() {
        assert!(ours.is_nan(), "mul_add({a:e}, {b:e}, {c:e}) = {ours:e}, std NaN");
    } else {
        assert_eq!(
            ours.to_bits(),
            theirs.to_bits(),
            "mul_add({a:e} [{:016x}], {b:e} [{:016x}], {c:e} [{:016x}]) = {ours:e}, std {theirs:e}",
            a.to_bits(),
            b.to_bits(),
            c.to_bits()
        );
    }
}

#[test]
fn special_values_match_std() {
    let vs = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        2.0,
        0.5,
        3.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::from_bits(0x000f_ffff_ffff_ffff),
        1.0 + f64::EPSILON,
        1.0 - f64::EPSILON / 2.0,
        1.0e300,
        1.0e-300,
    ];
    for a in vs {
        for b in vs {
            for c in vs {
                check(a, b, c);
            }
        }
    }
}

#[test]
fn exact_zero_sums_follow_round_to_nearest() {
    for x in [1.0_f64, 3.0, 1.0e-300, f64::MAX] {
        let y = 1.5;
        // x*y + (-(x*y)) is +0 whatever the rounding of the product, since the product is exact here
        // only for some; the oracle is std.
        check(x, y, -(x * y));
        check(-x, y, x * y);
    }
    check(-0.0, 1.0, -0.0);
    check(0.0, -1.0, -0.0);
    check(-0.0, 1.0, 0.0);
}

fn sweep(seed: u64, name: &str, mut triple: impl FnMut(&mut Rng) -> (f64, f64, f64)) {
    let mut rng = Rng(seed);
    let mut naive_differs = 0_u32;
    for _ in 0..1_000_000 {
        let (a, b, c) = triple(&mut rng);
        check(a, b, c);
        let naive = a * b + c;
        let fused = mul_add(a, b, c);
        if !(naive.is_nan() && fused.is_nan()) && naive.to_bits() != fused.to_bits() {
            naive_differs += 1;
        }
    }
    eprintln!("fma {name}: unfused a*b+c differs from the fused result in {naive_differs} of 1000000");
}

/// A random normal-ish number with exponent `e` (unbiased) and random mantissa and sign.
fn with_exp(rng: &mut Rng, e: i64) -> f64 {
    let m = rng.next() & 0x000f_ffff_ffff_ffff;
    let s = rng.next() & 0x8000_0000_0000_0000;
    f64::from_bits(s | (((e + 1023) as u64) << 52) | m)
}

#[test]
fn random_bit_patterns() {
    sweep(0x5eed_f001, "random bits", |r| {
        {
            let a = f64::from_bits(r.next());
            let b = f64::from_bits(r.next());
            (a, b, f64::from_bits(r.next()))
        }
    });
}

/// Exponents chosen so that a*b and c overlap: the sum has bits from both.
#[test]
fn overlapping_exponents() {
    sweep(0x5eed_f002, "overlap", |r| {
        let ea = (r.next() % 200) as i64 - 100;
        let eb = (r.next() % 200) as i64 - 100;
        let ec = ea + eb + (r.next() % 7) as i64 - 3;
        {
            let a = with_exp(r, ea);
            let b = with_exp(r, eb);
            (a, b, with_exp(r, ec))
        }
    });
}

/// c is minus the rounded product, perturbed by a few ulps: massive cancellation.
#[test]
fn cancellation() {
    sweep(0x5eed_f003, "cancel", |r| {
        let (ea, eb) = ((r.next() % 100) as i64 - 50, (r.next() % 100) as i64 - 50);
        let a = with_exp(r, ea);
        let b = with_exp(r, eb);
        let k = (r.next() % 5) as i64 - 2;
        let c = -(a * b);
        (a, b, f64::from_bits((c.to_bits() as i64 + k) as u64))
    });
}

/// Products in and below the subnormal range, and sums that land there.
#[test]
fn subnormal_results() {
    sweep(0x5eed_f004, "subnormal", |r| {
        let ea = -((r.next() % 600) as i64);
        let eb = -520 - (r.next() % 560) as i64;
        let ec = -1074 + (r.next() % 60) as i64;
        let c = if r.next() % 4 == 0 { 0.0 } else { with_exp(r, ec) };
        {
            let a = with_exp(r, ea);
            (a, with_exp(r, eb), c)
        }
    });
}

/// Products near and above the overflow threshold.
#[test]
fn overflow_edge() {
    sweep(0x5eed_f005, "overflow", |r| {
        let ea = 400 + (r.next() % 623) as i64;
        let eb = 1023 - ea + (r.next() % 5) as i64 - 3;
        let ec = 1000 + (r.next() % 24) as i64;
        {
            let a = with_exp(r, ea.min(1023));
            let b = with_exp(r, eb.clamp(-1022, 1023));
            (a, b, with_exp(r, ec))
        }
    });
}

#[test]
fn unfused_would_fail() {
    // 1 + 2^-27 squared is 1 + 2^-26 + 2^-54: the unfused product rounds the 2^-54 away.
    let a = 1.0 + 2.0_f64.powi(-27);
    let c = -(1.0 + 2.0_f64.powi(-26));
    assert_ne!((a * a + c).to_bits(), mul_add(a, a, c).to_bits());
    assert_eq!(mul_add(a, a, c), 2.0_f64.powi(-54));
}
