extern crate std;

#[test]
fn exp_relative_error_on_supported_interval() {
    let mut maximum = 0.0_f64;
    for at in 0..=30_000 {
        let x = -30.0_f32 + at as f32 * 0.001_f32;
        let exact = core::primitive::f64::from(x).exp();
        let relative = ((core::primitive::f64::from(super::exp(x)) - exact) / exact).abs();
        maximum = maximum.max(relative);
    }
    assert!(maximum < 2.0e-6, "maximum relative error was {maximum:e}");
}

#[test]
fn exp_underflows_to_zero_below_the_normal_range() {
    for x in [-87.5_f32, -90.0, -100.0, -1.0e4, f32::NEG_INFINITY] {
        assert_eq!(super::exp(x), 0.0, "exp({x})");
    }
    let near = super::exp(-86.9_f32);
    let exact = core::primitive::f64::from(-86.9_f32).exp();
    // Outside [-30, 0] no 2e-6 claim holds (measured 3.1e-6 here); the point is only that it is finite and close.
    assert!((core::primitive::f64::from(near) - exact).abs() / exact < 1.0e-5, "exp(-86.9) = {near:e}, exact {exact:e}");
}

#[test]
fn max_min_ordinary_values_and_argument_order() {
    assert_eq!(super::max(1.0, 2.0), 2.0);
    assert_eq!(super::max(2.0, 1.0), 2.0);
    assert_eq!(super::min(1.0, 2.0), 1.0);
    assert_eq!(super::min(2.0, 1.0), 1.0);
    assert_eq!(super::max(-3.0, -2.0), -2.0);
    assert_eq!(super::min(-3.0, -2.0), -3.0);
    assert_eq!(super::max(-2.0, 3.0), 3.0);
    assert_eq!(super::min(-2.0, 3.0), -2.0);
}

#[test]
fn max_min_zero_signs_are_fixed() {
    let pos = 0.0_f32.to_bits();
    let neg = (-0.0_f32).to_bits();
    // max: +0 if either argument is +0, else -0. min: -0 if either argument is -0, else +0.
    for (a, b) in [(0.0_f32, 0.0_f32), (0.0, -0.0), (-0.0, 0.0), (-0.0, -0.0)] {
        let want_max = if a.is_sign_positive() || b.is_sign_positive() { pos } else { neg };
        let want_min = if a.is_sign_negative() || b.is_sign_negative() { neg } else { pos };
        assert_eq!(super::max(a, b).to_bits(), want_max, "max({a:?}, {b:?})");
        assert_eq!(super::max(b, a).to_bits(), want_max, "max({b:?}, {a:?})");
        assert_eq!(super::min(a, b).to_bits(), want_min, "min({a:?}, {b:?})");
        assert_eq!(super::min(b, a).to_bits(), want_min, "min({b:?}, {a:?})");
    }
}

#[test]
fn max_min_nan_in_either_position() {
    for nan in [f32::NAN, -f32::NAN] {
        assert_eq!(super::max(nan, 1.0), 1.0);
        assert_eq!(super::max(1.0, nan), 1.0);
        assert_eq!(super::min(nan, -1.0), -1.0);
        assert_eq!(super::min(-1.0, nan), -1.0);
        assert!(super::max(nan, f32::NAN).is_nan());
        assert!(super::min(f32::NAN, nan).is_nan());
    }
}

#[test]
fn max_min_infinities() {
    assert_eq!(super::max(f32::INFINITY, 1.0), f32::INFINITY);
    assert_eq!(super::max(f32::NEG_INFINITY, 1.0), 1.0);
    assert_eq!(super::max(f32::INFINITY, f32::NEG_INFINITY), f32::INFINITY);
    assert_eq!(super::min(f32::NEG_INFINITY, 1.0), f32::NEG_INFINITY);
    assert_eq!(super::min(f32::INFINITY, 1.0), 1.0);
    assert_eq!(super::min(f32::INFINITY, f32::NEG_INFINITY), f32::NEG_INFINITY);
}

#[test]
fn max_min_agree_with_core_off_zero() {
    // Same as the f64 test: off zero, and off NaN, core's max and min are fixed bit for bit.
    let mut state = 0x5eed_0003_u64;
    let mut next = || {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (state >> 32) as u32
    };
    let mut checked = 0_u32;
    for _ in 0..200_000 {
        let a = f32::from_bits(next());
        let b = f32::from_bits(next());
        if a.is_nan() || b.is_nan() || (a == 0.0 && b == 0.0) {
            continue;
        }
        assert_eq!(super::max(a, b).to_bits(), a.max(b).to_bits(), "max({a:e}, {b:e})");
        assert_eq!(super::min(a, b).to_bits(), a.min(b).to_bits(), "min({a:e}, {b:e})");
        checked += 1;
    }
    assert!(checked > 190_000, "only {checked} pairs checked");
}
