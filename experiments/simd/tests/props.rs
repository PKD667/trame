//! Property tests: the lane kernels against the scalar reference, bitwise, on seeded random
//! inputs. The seeds are 0..SEEDS; a failure names its seed, kernel, sizes and first differing slot.
mod common;
use common::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use trame_simd_experiment::simd::*;
use trame_simd_experiment::*;

const SEEDS: u64 = 6000;

fn xi(r: &mut Rng, keys: &[u32]) -> Vec<Xi> {
    keys.iter().map(|&k| Xi { index: k, x: r.f32_any() }).collect()
}

fn msg(e: Box<dyn std::any::Any + Send>) -> String {
    match e.downcast::<String>() {
        Ok(s) => *s,
        Err(e) => e.downcast::<&str>().map(|s| s.to_string()).unwrap_or_else(|_| "?".into()),
    }
}

#[derive(Default)]
struct Tally {
    cases: u64,
    strict_mismatch: u64,
}

fn cmp_f32(t: &mut Tally, name: &str, seed: u64, a: &[f32], b: &[f32]) {
    t.cases += 1;
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        assert!(same(x, y), "{name} seed {seed}: slot {i}: ref {:#010x} simd {:#010x}", x.to_bits(), y.to_bits());
        if !strict(x, y) {
            t.strict_mismatch += 1;
        }
    }
}

#[test]
fn f32_kernels_match_the_scalar_fold() {
    if !has_avx2() {
        eprintln!("no avx2: skipped");
        return;
    }
    let (mut ta, mut tas, mut tp, mut tw, mut tr) = (Tally::default(), Tally::default(), Tally::default(), Tally::default(), Tally::default());
    for seed in 0..SEEDS {
        let mut r = Rng::new(seed);
        let cap = if r.below(4) == 0 { 300 } else { 70 };
        let n = 1 + r.below(cap);
        let cap = if r.below(8) == 0 { 9 } else { 400 };
        let len = r.below(cap);
        let ks = keys(&mut r, len, n);
        let items = xi(&mut r, &ks);
        let init: Vec<f32> = (0..n).map(|_| r.f32_any()).collect();
        let cx = Cx { a: r.f32_any(), t: r.unit() };
        let cp = Cp { c: core::array::from_fn(|_| r.f32_finite()) };

        let (mut a, mut b) = (init.clone(), init.clone());
        axpy_ref(&cx, &items, &mut a);
        axpy_simd(&cx, &items, &mut b);
        cmp_f32(&mut ta, "axpy", seed, &a, &b);

        let (mut a, mut b) = (init.clone(), init.clone());
        axpy_sel_ref(&cx, &items, &mut a);
        axpy_sel_simd(&cx, &items, &mut b);
        cmp_f32(&mut tas, "axpy_sel", seed, &a, &b);

        let (mut a, mut b) = (init.clone(), init.clone());
        poly_ref(&cp, &items, &mut a);
        poly_simd(&cp, &items, &mut b);
        cmp_f32(&mut tp, "poly", seed, &a, &b);

        let (mut a, mut b) = (init.clone(), init.clone());
        sum32_ref(&items, &mut a);
        sum32_simd_window(&items, &mut b);
        cmp_f32(&mut tw, "sum32 window", seed, &a, &b);

        let mut c = init.clone();
        sum32_simd(&items, &mut c); // register kernel when n <= 64
        cmp_f32(&mut tr, "sum32 dispatch", seed, &a, &c);
    }
    println!("seeds 0..{SEEDS}; cases, strict-bit mismatches that were NaN-vs-NaN payload only:");
    for (n, t) in [("axpy", &ta), ("axpy_sel", &tas), ("poly", &tp), ("sum32 window", &tw), ("sum32 reg/window", &tr)] {
        println!("  {n}: {} cases, {} slots equal only up to NaN payload", t.cases, t.strict_mismatch);
    }
}

#[test]
fn u64_kernels_match_the_scalar_fold_including_overflow() {
    if !has_avx2() {
        return;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let (mut cases, mut aborts) = (0u64, 0u64);
    for seed in 0..SEEDS {
        let mut r = Rng::new(seed ^ 0xABCD);
        let cap = if r.below(4) == 0 { 200 } else { 40 };
        let n = 1 + r.below(cap);
        let len = r.below(300);
        let ks = keys(&mut r, len, n);
        let big = r.below(3);
        let val = |r: &mut Rng| -> u64 {
            match big {
                0 => r.below(1000) as u64,
                1 => r.next() >> (8 + r.below(40)),
                _ => if r.below(30) == 0 { u64::MAX - r.below(5) as u64 } else { r.next() >> 20 },
            }
        };
        let items: Vec<Kv64> = ks.iter().map(|&k| Kv64 { index: k, _p: 0, v: val(&mut r) }).collect();
        let init: Vec<u64> = (0..n).map(|_| val(&mut r)).collect();
        for (name, f) in [
            ("sum64 window", sum64_simd_window as fn(&[Kv64], &mut [u64])),
            ("sum64 dispatch", sum64_simd as fn(&[Kv64], &mut [u64])),
        ] {
            let (mut a, mut b) = (init.clone(), init.clone());
            let ra = catch_unwind(AssertUnwindSafe(|| sum64_ref(&items, &mut a)));
            let rb = catch_unwind(AssertUnwindSafe(|| f(&items, &mut b)));
            cases += 1;
            match (ra, rb) {
                (Ok(()), Ok(())) => {}
                (Err(x), Err(y)) => {
                    aborts += 1;
                    let (x, y) = (msg(x), msg(y));
                    assert_eq!(x, y, "{name} seed {seed}: abort message");
                }
                (x, y) => panic!("{name} seed {seed}: ref aborted={} simd aborted={}", x.is_err(), y.is_err()),
            }
            assert_eq!(a, b, "{name} seed {seed}: slots (also after an abort)");
        }
    }
    let _ = std::panic::take_hook();
    println!("u64: seeds 0..{SEEDS}, {cases} cases, {aborts} aborted (overflow) with equal message and equal slots");
}

#[test]
fn a_bad_key_aborts_with_the_scalar_message_and_no_item_runs() {
    if !has_avx2() {
        return;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let mut cases = 0;
    for seed in 0..2000u64 {
        let mut r = Rng::new(seed ^ 0x5555);
        let n = 1 + r.below(40);
        let len = 1 + r.below(100);
        let mut ks = keys(&mut r, len, n);
        for _ in 0..1 + r.below(3) {
            let p = r.below(len);
            ks[p] = (n + r.below(5)) as u32 + if r.below(4) == 0 { 0x8000_0000 } else { 0 };
        }
        let items = xi(&mut r, &ks);
        let init: Vec<f32> = (0..n).map(|_| r.unit()).collect();
        let (mut a, mut b, mut c) = (init.clone(), init.clone(), init.clone());
        let ra = catch_unwind(AssertUnwindSafe(|| sum32_ref(&items, &mut a))).unwrap_err();
        let rb = catch_unwind(AssertUnwindSafe(|| sum32_simd_window(&items, &mut b))).unwrap_err();
        let rc = catch_unwind(AssertUnwindSafe(|| sum32_simd(&items, &mut c))).unwrap_err();
        let m = msg(ra);
        assert_eq!(m, msg(rb), "seed {seed}");
        assert_eq!(m, msg(rc), "seed {seed}");
        assert!(init.iter().zip(&a).all(|(x, y)| strict(*x, *y)), "ref ran an item");
        assert!(init.iter().zip(&b).all(|(x, y)| strict(*x, *y)), "window ran an item");
        assert!(init.iter().zip(&c).all(|(x, y)| strict(*x, *y)), "reg ran an item");
        cases += 1;
    }
    // u64 too
    for seed in 0..500u64 {
        let mut r = Rng::new(seed);
        let n = 1 + r.below(30);
        let len = 1 + r.below(60);
        let mut ks = keys(&mut r, len, n);
        let p = r.below(len);
        ks[p] = n as u32 + r.below(3) as u32;
        let items: Vec<Kv64> = ks.iter().map(|&k| Kv64 { index: k, _p: 0, v: 1 }).collect();
        let (mut a, mut b) = (vec![7u64; n], vec![7u64; n]);
        let ra = catch_unwind(AssertUnwindSafe(|| sum64_ref(&items, &mut a))).unwrap_err();
        let rb = catch_unwind(AssertUnwindSafe(|| sum64_simd(&items, &mut b))).unwrap_err();
        assert_eq!(msg(ra), msg(rb));
        assert_eq!((a, b), (vec![7; n], vec![7; n]));
    }
    let _ = std::panic::take_hook();
    println!("bad key: {cases} f32 cases + 500 u64 cases, same message, no slot written");
}
