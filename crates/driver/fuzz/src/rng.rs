//! Deterministic randomness: xorshift64* (Vigna), seeded by splitmix64 so that nearby seeds differ.
//! Every program is generated from `mix(seed, index)`, so any one program can be regenerated alone.

pub fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The seed of program `index` in the run seeded `seed`.
pub fn mix(seed: u64, index: u64) -> u64 {
    splitmix(seed ^ splitmix(index.wrapping_add(1)))
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(splitmix(seed).max(1));
        r.next();
        r
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0)");
        self.next() % n
    }

    pub fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }

    /// An index drawn with the given integer weights (a zero weight is never drawn).
    pub fn weighted(&mut self, weights: &[u64]) -> usize {
        let total: u64 = weights.iter().sum();
        assert!(total > 0, "weighted: all weights are zero");
        let mut t = self.below(total);
        for (i, w) in weights.iter().enumerate() {
            if t < *w {
                return i;
            }
            t -= w;
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn index_changes_seed() {
        assert_ne!(mix(1, 0), mix(1, 1));
        assert_ne!(mix(1, 0), mix(2, 0));
    }
}
