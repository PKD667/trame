//! Tests of `Bounded` and `BoundedHeap` against std (`Vec`, `BinaryHeap`) under random operation sequences,
//! and of the rule that the buffer never moves: its address and capacity are the same after `new` as after
//! every operation.

extern crate std;

use super::{Bounded, BoundedHeap, Full};
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Reverse;
use core::fmt::Debug;
use std::collections::BinaryHeap;

/// Deterministic generator (64-bit LCG, wrapping), the same pattern as src/math/f64/tests.rs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// A value in `[0, n)` from the top bits. `n > 0`.
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 33) as usize) % n
    }
}

#[test]
fn full_is_a_unit_type_with_a_display() {
    assert_eq!(alloc::format!("{}", Full), "full: at capacity");
    assert_eq!(Full, Full.clone());
}

#[test]
fn push_on_full_answers_full_and_keeps_the_contents() {
    let mut b = Bounded::new(2);
    assert_eq!(b.push(1_u32), Ok(()));
    assert_eq!(b.push(2), Ok(()));
    assert!(b.is_full());
    assert_eq!(b.push(3), Err(Full));
    assert_eq!(&*b, &[1, 2]);
    assert_eq!(b.len(), 2);
    assert_eq!(b.cap(), 2);
}

#[test]
fn push_owned_hands_the_value_back_when_full() {
    let mut b = Bounded::new(1);
    assert_eq!(b.push_owned(String::from("a")), Ok(()));
    assert_eq!(b.push_owned(String::from("b")), Err(String::from("b")));
    assert_eq!(b.as_slice(), &[String::from("a")]);
}

#[test]
fn extend_is_all_or_nothing() {
    let mut b = Bounded::new(3);
    assert_eq!(b.push(7_u32), Ok(()));
    assert_eq!(b.extend_from_slice(&[1, 2, 3]), Err(Full));
    assert_eq!(&*b, &[7]);
    assert_eq!(b.extend_from_slice(&[1, 2]), Ok(()));
    assert_eq!(&*b, &[7, 1, 2]);
}

#[test]
fn zero_capacity_works() {
    let mut b = Bounded::<u32>::new(0);
    assert_eq!(b.cap(), 0);
    assert!(b.is_empty() && b.is_full());
    assert_eq!(b.push(1), Err(Full));
    assert_eq!(b.extend_from_slice(&[]), Ok(()));
    assert_eq!(b.extend_from_slice(&[1]), Err(Full));
    assert_eq!(b.pop(), None);
    assert_eq!(b.swap_remove(0), None);
    assert_eq!(b.first(), None);

    let mut h = BoundedHeap::<u32>::new(0);
    assert_eq!(h.push(1), Err(Full));
    assert_eq!(h.pop(), None);
    assert_eq!(h.peek(), None);
    assert_eq!(h.push_pop(5), 5);
    assert!(h.is_empty());
}

#[test]
fn zero_sized_values_work() {
    let mut b = Bounded::<()>::new(5);
    for _ in 0..5 {
        assert_eq!(b.push(()), Ok(()));
    }
    assert_eq!(b.push(()), Err(Full));
    assert_eq!(b.extend_from_slice(&[(), ()]), Err(Full));
    assert_eq!(b.len(), 5);
    assert_eq!(b.swap_remove(4), Some(()));
    assert_eq!(b.pop(), Some(()));
    b.truncate(1);
    assert_eq!(b.len(), 1);
    b.clear();
    assert!(b.is_empty());

    let mut big = Bounded::<()>::new(usize::MAX);
    assert_eq!(big.cap(), usize::MAX);
    assert_eq!(big.push(()), Ok(()));
    assert!(!big.is_full());

    let mut h = BoundedHeap::<()>::new(3);
    assert_eq!(h.push(()), Ok(()));
    assert_eq!(h.push_pop(()), ());
    assert_eq!(h.pop(), Some(()));
    assert_eq!(h.pop(), None);
}

#[test]
fn heap_on_full_answers_full_and_keeps_the_heap() {
    let mut h = BoundedHeap::new(2);
    assert_eq!(h.push(3_u32), Ok(()));
    assert_eq!(h.push(9), Ok(()));
    assert!(h.is_full());
    assert_eq!(h.push(100), Err(Full));
    assert_eq!(h.peek(), Some(&9));
    assert_eq!(h.len(), 2);
    // push_pop never fails: 100 is larger than the top, so it comes straight back.
    assert_eq!(h.push_pop(100), 100);
    assert_eq!(h.push_pop(4), 9);
    assert_eq!(h.len(), 2);
    assert_eq!(h.pop(), Some(4));
    assert_eq!(h.pop(), Some(3));
    assert_eq!(h.pop(), None);
}

#[test]
fn heap_ordering_is_as_documented() {
    let mut h = BoundedHeap::new(4);
    for x in [5_u32, 1, 4, 1] {
        h.push(x).unwrap();
    }
    assert_eq!(h.as_slice()[0], 5);
    let mut seen = Vec::new();
    while let Some(x) = h.pop() {
        seen.push(x);
    }
    assert_eq!(seen, [5, 4, 1, 1]);
}

/// The buffer's address and capacity, which must not change.
fn buffer_of<T>(b: &Bounded<T>) -> (*const T, usize) {
    (b.as_slice().as_ptr(), b.buf.capacity())
}

/// Random operations on `Bounded<u32>` against `Vec<u32>` with the same cap rule. Checks, after every
/// operation, that the contents agree and that the buffer has not moved (`never allocates`).
#[test]
fn vec_matches_the_model_and_never_moves() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..100 {
        let cap = rng.below(9);
        let mut b = Bounded::<u32>::new(cap);
        let start = buffer_of(&b);
        let mut m: Vec<u32> = Vec::new();
        for _ in 0..1_000 {
            match rng.below(8) {
                0 | 1 => {
                    let v = rng.below(16) as u32;
                    if m.len() < cap {
                        m.push(v);
                        assert_eq!(b.push(v), Ok(()));
                    } else {
                        assert_eq!(b.push(v), Err(Full));
                    }
                }
                2 => assert_eq!(b.pop(), m.pop()),
                3 => {
                    if rng.below(8) == 0 {
                        b.clear();
                        m.clear();
                    }
                }
                4 => {
                    let n = rng.below(m.len() + 2);
                    b.truncate(n);
                    m.truncate(n);
                }
                5 => {
                    let i = rng.below(m.len() + 2);
                    let want = if i < m.len() { Some(m.swap_remove(i)) } else { None };
                    assert_eq!(b.swap_remove(i), want);
                }
                6 => {
                    let k = rng.below(4);
                    let xs: Vec<u32> = (0..k).map(|_| rng.below(16) as u32).collect();
                    if m.len() + k <= cap {
                        m.extend_from_slice(&xs);
                        assert_eq!(b.extend_from_slice(&xs), Ok(()));
                    } else {
                        assert_eq!(b.extend_from_slice(&xs), Err(Full));
                    }
                }
                _ => {
                    let i = rng.below(m.len() + 2);
                    assert_eq!(b.get(i), m.get(i));
                    if let Some(x) = b.get_mut(i) {
                        *x = x.wrapping_add(1);
                        m[i] = m[i].wrapping_add(1);
                    }
                    assert_eq!(b.first(), m.first());
                    assert_eq!(b.last(), m.last());
                }
            }
            assert_eq!(&*b, m.as_slice());
            assert_eq!(b.as_mut_slice().len(), m.len());
            assert_eq!(b.len(), m.len());
            assert_eq!(b.is_empty(), m.is_empty());
            assert_eq!(b.is_full(), m.len() == cap);
            assert_eq!(buffer_of(&b), start, "the buffer moved or grew");
        }
    }
}

/// Checks the max-heap property and that the heap holds exactly the model's values.
fn assert_heap<T: Ord + Copy + Debug>(h: &BoundedHeap<T>, m: &BinaryHeap<T>) {
    let s = h.as_slice();
    for i in 1..s.len() {
        assert!(s[(i - 1) / 2] >= s[i], "heap property broken at {i}: {s:?}");
    }
    let mut got = s.to_vec();
    got.sort();
    let mut want = m.clone().into_vec();
    want.sort();
    assert_eq!(got, want);
    assert_eq!(h.peek(), m.peek());
    assert_eq!(h.len(), m.len());
}

/// Random operations on `BoundedHeap<T>` against `BinaryHeap<T>` with the same cap rule. Keys are drawn
/// from a small range so duplicates are common.
fn heap_matches_the_model<T: Ord + Copy + Debug>(seed: u64, key: fn(u32) -> T) {
    let mut rng = Rng(seed);
    for _ in 0..50 {
        let cap = rng.below(9);
        let mut h = BoundedHeap::<T>::new(cap);
        let start = buffer_of(&h.items);
        let mut m = BinaryHeap::<T>::new();
        for _ in 0..400 {
            match rng.below(5) {
                0 | 1 => {
                    let v = key(rng.below(8) as u32);
                    if m.len() < cap {
                        m.push(v);
                        assert_eq!(h.push(v), Ok(()));
                    } else {
                        assert_eq!(h.push(v), Err(Full));
                    }
                }
                2 => assert_eq!(h.pop(), m.pop()),
                3 => {
                    // push_pop is push then pop, and never fails.
                    let v = key(rng.below(8) as u32);
                    m.push(v);
                    let want = m.pop().unwrap();
                    assert_eq!(h.push_pop(v), want);
                }
                _ => {
                    if rng.below(10) == 0 {
                        h.clear();
                        m.clear();
                    }
                }
            }
            assert_heap(&h, &m);
            assert_eq!(buffer_of(&h.items), start, "the buffer moved or grew");
        }
        while let Some(want) = m.pop() {
            assert_eq!(h.pop(), Some(want));
        }
        assert_eq!(h.pop(), None);
    }
}

#[test]
fn heap_matches_binary_heap() {
    for seed in 1..=20 {
        heap_matches_the_model(seed, |x| x);
    }
}

#[test]
fn min_heap_through_reverse_matches_binary_heap() {
    for seed in 100..=110 {
        heap_matches_the_model(seed, Reverse);
    }
}

/// The 100k-operation check of the claim in mod.rs: the buffer's address and capacity never change.
#[test]
fn a_hundred_thousand_operations_never_move_the_buffer() {
    let mut rng = Rng(42);
    let mut b = Bounded::<u64>::new(64);
    let mut h = BoundedHeap::<u64>::new(64);
    let (bp, hp) = (buffer_of(&b), buffer_of(&h.items));
    for _ in 0..100_000 {
        let v = rng.next();
        match rng.below(6) {
            0 => {
                let _ = b.push(v);
                let _ = h.push(v);
            }
            1 => {
                b.pop();
                h.pop();
            }
            2 => {
                let _ = b.extend_from_slice(&[v, v ^ 1, v.rotate_left(7)]);
                h.push_pop(v);
            }
            3 => {
                b.swap_remove(rng.below(70));
                b.truncate(rng.below(70));
            }
            4 => {
                if rng.below(50) == 0 {
                    b.clear();
                    h.clear();
                }
            }
            _ => {
                let i = rng.below(70);
                if let Some(x) = b.get_mut(i) {
                    *x ^= v;
                }
            }
        }
        assert_eq!(buffer_of(&b), bp);
        assert_eq!(buffer_of(&h.items), hp);
        assert!(b.len() <= 64 && h.len() <= 64);
    }
}
