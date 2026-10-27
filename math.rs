//! Numeric functions of the crate: pure code at the item level, not effects (spec.md 7). Each one is
//! defined by this code on every backend, so `trame::math::*` is never backend-specific.

/// `e^x`, defined by this code on every backend: there is no backend-specific version, so every
/// backend returns the same bits. Inputs in `[-30, 0]`, relative error below 2e-6 against `f64::exp`.
pub fn exp(x: f32) -> f32 {
    let n = (x / core::f32::consts::LN_2) as i32;
    let r = x - (n as f32) * core::f32::consts::LN_2;
    let p = 1.0_f32
        + r * (1.0_f32
            + r * (0.5_f32
                + r * (0.166_666_67_f32
                    + r * (0.041_666_668_f32
                        + r * (0.008_333_334_f32
                            + r * (0.001_388_888_9_f32
                                + r * (0.000_198_412_7_f32
                                    + r * (0.000_024_801_588_f32
                                        + r * 0.000_002_755_732_f32))))))));
    p * f32::from_bits(((n + 127) as u32) << 23)
}

#[cfg(test)]
mod tests {
    extern crate std;
    #[test]
    fn exp_relative_error_on_supported_interval() {
        let mut maximum = 0.0_f64;
        for at in 0..=30_000 {
            let x = -30.0_f32 + at as f32 * 0.001_f32;
            let exact = f64::from(x).exp();
            let relative = ((f64::from(super::exp(x)) - exact) / exact).abs();
            maximum = maximum.max(relative);
        }
        assert!(maximum < 2.0e-6, "maximum relative error was {maximum:e}");
    }
}
