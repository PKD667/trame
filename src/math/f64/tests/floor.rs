//! `trunc`, `floor`, `ceil`, `fract`, `round_ties_even`: bit-for-bit equal to std (these are exact
//! operations, so there is nothing to measure in ulps), over special values and over random bit
//! patterns, which cover every exponent, the subnormals and the integers above 2^52.

extern crate std;

use super::super::{ceil, floor, fract, round_ties_even, trunc};
use super::Rng;

type Pair = (&'static str, fn(f64) -> f64, fn(f64) -> f64);

const FUNCTIONS: [Pair; 5] = [
    ("trunc", trunc, f64::trunc),
    ("floor", floor, f64::floor),
    ("ceil", ceil, f64::ceil),
    ("fract", fract, f64::fract),
    ("round_ties_even", round_ties_even, f64::round_ties_even),
];

fn same(name: &str, ours: f64, theirs: f64, x: f64) {
    if theirs.is_nan() {
        assert!(ours.is_nan(), "{name}({x:e}): got {ours:e}, std gives NaN");
    } else {
        assert_eq!(ours.to_bits(), theirs.to_bits(), "{name}({x:e}) = {ours:e}, std {theirs:e}");
    }
}

#[test]
fn special_values_match_std() {
    let two52 = 4_503_599_627_370_496.0_f64;
    let xs = [
        0.0,
        -0.0,
        0.3,
        -0.3,
        0.5,
        -0.5,
        0.7,
        -0.7,
        1.0,
        -1.0,
        1.5,
        -1.5,
        2.5,
        -2.5,
        3.5,
        0.499_999_999_999_999_94,
        -0.499_999_999_999_999_94,
        two52 - 0.5,
        two52 - 1.5,
        two52,
        two52 + 1.0,
        -two52 + 0.5,
        9_007_199_254_740_993.0,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ];
    for x in xs {
        for (name, ours, theirs) in FUNCTIONS {
            same(name, ours(x), theirs(x), x);
        }
    }
}

#[test]
fn halfway_cases_of_round_ties_even() {
    for (x, want) in [(0.5, 0.0), (1.5, 2.0), (2.5, 2.0), (3.5, 4.0), (-0.5, -0.0), (-1.5, -2.0), (-2.5, -2.0), (0.500_000_000_000_000_1, 1.0)] {
        assert_eq!(round_ties_even(x).to_bits(), f64::to_bits(want), "round_ties_even({x})");
    }
}

#[test]
fn random_bit_patterns_match_std() {
    let mut rng = Rng(0x5eed_00f1);
    for _ in 0..400_000 {
        let x = f64::from_bits(rng.next());
        for (name, ours, theirs) in FUNCTIONS {
            same(name, ours(x), theirs(x), x);
        }
    }
}

/// Random values with a fractional part (exponents 0..52 only), where the interesting branches live.
#[test]
fn values_with_fractions_match_std() {
    let mut rng = Rng(0x5eed_00f2);
    for _ in 0..400_000 {
        let e = rng.next() % 54;
        let x = f64::from_bits(((1023 - 2 + e) << 52) | (rng.next() & 0x800f_ffff_ffff_ffff));
        for (name, ours, theirs) in FUNCTIONS {
            same(name, ours(x), theirs(x), x);
        }
    }
}
