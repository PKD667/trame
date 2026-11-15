//! Exhaustive check of the index logic of the window kernels on small key alphabets. The kernels'
//! behaviour depends on the keys only through which are equal, which are consecutive, and the
//! tail; Bell(8) = 4140 equality patterns are all reached by 8 keys over 8 symbols.
mod common;
use common::*;
use trame_simd_experiment::plan::{Class, classify};
use trame_simd_experiment::simd::*;
use trame_simd_experiment::*;

fn all_tuples<const N: usize>(alphabet: &[u32], mut f: impl FnMut(&[u32; N])) -> u64 {
    let a = alphabet.len();
    let mut idx = [0usize; N];
    let mut count = 0u64;
    loop {
        let keys: [u32; N] = core::array::from_fn(|i| alphabet[idx[i]]);
        f(&keys);
        count += 1;
        let mut p = 0;
        loop {
            if p == N {
                return count;
            }
            idx[p] += 1;
            if idx[p] < a {
                break;
            }
            idx[p] = 0;
            p += 1;
        }
    }
}

#[test]
fn classify8_is_the_specification() {
    if !has_avx2() {
        return;
    }
    let alphabets: [&[u32]; 3] = [
        &[0, 1, 2, 3, 4, 5, 6, 7],
        &[3, 4, 5, 6, 7, 8, 9, 10],
        &[0, 1, u32::MAX - 1, u32::MAX, 1 << 31, (1 << 31) + 1, 5, 6],
    ];
    let mut seen = [0u64; 3];
    for al in alphabets {
        let n = all_tuples::<8>(al, |k| {
            let want = classify(k);
            let got = unsafe { classify8(k) };
            assert_eq!(got, want, "{k:?}");
            seen[want as usize] += 1;
        });
        assert_eq!(n, 8u64.pow(8));
    }
    println!(
        "classify8 = classify on 3 x 8^8 key tuples; contiguous {} distinct {} conflict {}",
        seen[0], seen[1], seen[2]
    );
    assert!(seen.iter().all(|&c| c > 0));
    let _ = Class::Conflict;
}

fn check_f32<const N: usize>(alphabet: &[u32], slots: usize, tag: &str) {
    let mut r = Rng::new(42);
    let mut cases = 0u64;
    all_tuples::<N>(alphabet, |k| {
        let items: Vec<Xi> = k.iter().map(|&i| Xi { index: i, x: r.f32_any() }).collect();
        let init: Vec<f32> = (0..slots).map(|_| r.f32_any()).collect();
        let (mut a, mut b, mut c) = (init.clone(), init.clone(), init.clone());
        sum32_ref(&items, &mut a);
        sum32_simd_window(&items, &mut b);
        sum32_simd(&items, &mut c);
        for i in 0..slots {
            assert!(same(a[i], b[i]) && same(a[i], c[i]), "{tag} {k:?} slot {i}");
        }
        cases += 1;
    });
    println!("{tag}: {cases} key tuples, window and dispatch kernels equal the scalar fold");
}

#[test]
fn f32_window_one_block() {
    if has_avx2() {
        check_f32::<8>(&[0, 1, 2, 3, 4, 5, 6, 7], 8, "f32 8 items, keys 0..8, n=8");
        check_f32::<8>(&[3, 4, 5, 6, 7, 8, 9, 10], 11, "f32 8 items, keys 3..11, n=11");
    }
}

#[test]
fn f32_window_block_and_tail() {
    if has_avx2() {
        check_f32::<12>(&[0, 1, 2, 3], 9, "f32 12 items, keys 0..4, n=9 (one block + tail 4)");
        check_f32::<9>(&[0, 1, 2, 3, 4, 5], 8, "f32 9 items, keys 0..6, n=8");
    }
}

#[test]
fn u64_window_and_reg() {
    if !has_avx2() {
        return;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let mut r = Rng::new(7);
    let mut cases = 0u64;
    // no overflow: 8 items over 8 keys, two blocks
    all_tuples::<8>(&[0, 1, 2, 3, 4, 5, 6, 7], |k| {
        let items: Vec<Kv64> = k.iter().map(|&i| Kv64 { index: i, _p: 0, v: r.next() >> 8 }).collect();
        let init: Vec<u64> = (0..8).map(|_| r.next() >> 8).collect();
        let (mut a, mut b, mut c) = (init.clone(), init.clone(), init.clone());
        sum64_ref(&items, &mut a);
        sum64_simd_window(&items, &mut b);
        sum64_simd(&items, &mut c);
        assert!(a == b && a == c, "{k:?}");
        cases += 1;
    });
    println!("u64 (no overflow): {cases} key tuples equal");
    // overflow: 6 items over 3 keys, values and slots from {0, 1, MAX}
    let vals = [0u64, 1, u64::MAX];
    let mut vcases = 0u64;
    let mut aborts = 0u64;
    all_tuples::<6>(&[0, 1, 2], |k| {
        all_tuples::<6>(&[0, 1, 2], |vi| {
            let items: Vec<Kv64> = (0..6).map(|j| Kv64 { index: k[j], _p: 0, v: vals[vi[j] as usize] }).collect();
            for init in [[0u64; 3], [u64::MAX; 3], [1, u64::MAX, 0]] {
                let (mut a, mut b, mut c) = (init.to_vec(), init.to_vec(), init.to_vec());
                let ra = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sum64_ref(&items, &mut a)));
                let rb = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sum64_simd_window(&items, &mut b)));
                let rc = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sum64_simd(&items, &mut c)));
                assert_eq!((ra.is_err(), ra.is_err()), (rb.is_err(), rc.is_err()), "{k:?} {vi:?} {init:?}");
                assert!(a == b && a == c, "{k:?} {vi:?} {init:?}: slots after abort");
                aborts += ra.is_err() as u64;
                vcases += 1;
            }
        });
    });
    let _ = std::panic::take_hook();
    println!("u64 (overflow): {vcases} cases, {aborts} abort; abort/no-abort and slots (also at the abort) equal");
}
