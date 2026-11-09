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
