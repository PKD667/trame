/// Portable primitives expressed as arithmetic so a backend can replace them at its boundary.
/// The approximation is for single-precision inputs in `[-30, 0]` and has relative error < 2e-6.
pub fn exp(x: f32) -> f32 {
    let n = (x / 0.693_147_2_f32) as i32;
    let r = x - (n as f32) * 0.693_147_2_f32;
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

#[cfg(all(test, not(feature = "nv")))]
mod tests {
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
