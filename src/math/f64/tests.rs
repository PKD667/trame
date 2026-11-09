//! Tests of the f64 functions. `consts` checks each constant against the bits in fdlibm's comments;
//! `special` checks special values; `sweep` measures the error against std; `hash` pins the bits.

extern crate std;

mod consts;
mod hash;
mod special;
mod sweep;

/// Deterministic generator (64-bit LCG, wrapping): the same sequence on every machine.
pub(super) struct Rng(pub u64);

impl Rng {
    pub(super) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// A uniform value in `[0, 1)` from the top 53 bits.
    pub(super) fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }
}

/// Distance in units in the last place between two non-NaN values; `+0` and `-0` are 0 apart.
pub(super) fn ulps(a: f64, b: f64) -> u64 {
    fn ordered(x: f64) -> i64 {
        let b = x.to_bits() as i64;
        if b < 0 { -(b & i64::MAX) } else { b }
    }
    ordered(a).abs_diff(ordered(b))
}
