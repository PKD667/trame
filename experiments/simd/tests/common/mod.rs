//! A seeded generator and the comparison used by the property tests. No dependencies: proptest
//! is not in the node's offline crate cache.
#![allow(dead_code)]

pub struct Rng(pub u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03);
        r.next();
        r
    }
    /// splitmix64
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) as u128 * n as u128 >> 53) as usize
    }
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
    /// A float that is mostly ordinary and sometimes one of the values where a lane could differ.
    pub fn f32_any(&mut self) -> f32 {
        match self.below(24) {
            0 => 0.0,
            1 => -0.0,
            2 => f32::from_bits(1 + self.below(0x7F_FFFF) as u32), // denormal
            3 => -f32::from_bits(1 + self.below(0x7F_FFFF) as u32),
            4 => f32::INFINITY,
            5 => f32::NEG_INFINITY,
            6 => f32::from_bits(0x7FC0_0000 | (self.next() as u32 & 0x3F_FFFF)), // quiet NaN, any payload
            7 => 3.0e38 * self.unit(),
            8 => 1.0e-38 * self.unit(),
            9 => (self.below(9) as f32) - 4.0,
            _ => self.unit() * 8.0,
        }
    }
    pub fn f32_finite(&mut self) -> f32 {
        loop {
            let x = self.f32_any();
            if x.is_finite() {
                return x;
            }
        }
    }
}

/// Equal as a bit pattern, or both NaN. Rust does not fix a NaN's payload (Reference, floating
/// point types), so the second clause is the contract; `strict` counts what the first clause alone
/// would have rejected.
pub fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}
pub fn strict(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

/// Key vectors of several shapes, over `n` slots.
pub fn keys(r: &mut Rng, len: usize, n: usize) -> Vec<u32> {
    let mode = r.below(7);
    let mut perm: Vec<u32> = (0..n as u32).collect();
    for i in (1..n).rev() {
        perm.swap(i, r.below(i + 1));
    }
    (0..len)
        .map(|i| match mode {
            0 => (i % n) as u32,                       // identity-like, wraps when len > n
            1 => perm[i % n],                          // a permutation, repeated
            2 => r.below(n) as u32,                    // uniform
            3 => ((r.below(1000) as f64 / 1000.0).powi(3) * n as f64) as usize as u32 % n as u32, // skew to key 0
            4 => (i / (1 + r.below(5)).max(1) % n) as u32, // runs
            5 => ((i / 8) * 8 + r.below(8)) as u32 % n as u32, // locally shuffled blocks
            _ => (r.below(2) * (n - 1)) as u32,        // two hot keys
        })
        .collect()
}
