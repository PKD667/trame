//! The graph of scale_graph and the recurrence it computes, shared with scale/baseline/graph.c, which
//! implements the same two functions in C and must give the same numbers.

/// splitmix64 (Steele, Lea and Flood, 2014): the generator of the offsets.
pub struct SplitMix(u64);

impl SplitMix {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// `degree` distinct offsets in 1..w, in the order drawn: o = 1 + draw % (w - 1), duplicates skipped.
/// The caller checks 1 <= degree <= w - 1; otherwise this does not end.
pub fn offsets(w: u64, degree: u64, seed: u64) -> Vec<u64> {
    let mut rng = SplitMix::new(seed);
    let mut offs = Vec::new();
    while (offs.len() as u64) < degree {
        let o = 1 + rng.draw() % (w - 1);
        if !offs.contains(&o) {
            offs.push(o);
        }
    }
    offs
}

/// `n` rounds of xorshift64 (Marsaglia, 2003) from state `x`. A worker's state starts at its index + 1.
pub fn rounds(mut x: u64, n: u64) -> u64 {
    for _ in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
    }
    x
}
