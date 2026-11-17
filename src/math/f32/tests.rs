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
