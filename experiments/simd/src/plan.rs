//! The index logic of the window kernels as a plain function, the specification the vector code is
//! checked against (tests/exhaustive.rs).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// `keys[j] == keys[0] + j` (wrapping, 32 bits): one contiguous load and store.
    Contiguous,
    /// Pairwise distinct, not contiguous: gather, then scalar stores.
    Distinct,
    /// Some key repeats: the window runs through the scalar item in input order.
    Conflict,
}

pub fn classify(keys: &[u32]) -> Class {
    if keys.iter().enumerate().all(|(j, &k)| k == keys[0].wrapping_add(j as u32)) {
        return Class::Contiguous;
    }
    for i in 0..keys.len() {
        for j in i + 1..keys.len() {
            if keys[i] == keys[j] {
                return Class::Conflict;
            }
        }
    }
    Class::Distinct
}
