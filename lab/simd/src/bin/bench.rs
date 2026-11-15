//! items/s of the scalar reference (`trame::__rt::parallel`) against the lane kernels.
//! Output: one TSV line per (case, size, variant): median over `runs` calls, each on a fresh copy
//! of the slots; the copy is not timed. Every variant's result is compared bit for bit with the
//! scalar result on the same input (column `eq`).
use std::hint::black_box;
use std::time::Instant;
use trame_simd_experiment::plan::{Class, classify};
use trame_simd_experiment::simd::*;
use trame_simd_experiment::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
    fn u01(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Clone, Copy)]
enum Dist {
    Ident,
    Perm,
    Uniform,
    Skew,
}
impl Dist {
    fn name(self) -> &'static str {
        match self {
            Dist::Ident => "ident",
            Dist::Perm => "perm",
            Dist::Uniform => "uniform",
            Dist::Skew => "skew",
        }
    }
}

fn make_keys(r: &mut Rng, len: usize, n: usize, d: Dist) -> Vec<u32> {
    match d {
        Dist::Ident => (0..len as u32).collect(),
        Dist::Perm => {
            let mut p: Vec<u32> = (0..len as u32).collect();
            for i in (1..len).rev() {
                p.swap(i, (r.next() % (i as u64 + 1)) as usize);
            }
            p
        }
        Dist::Uniform => (0..len).map(|_| (r.next() % n as u64) as u32).collect(),
        // u^3: half the items on the first n/8 keys when n = 8 is half on key 0
        Dist::Skew => (0..len).map(|_| ((r.u01().powi(3) * n as f64) as usize).min(n - 1) as u32).collect(),
    }
}

fn median(mut v: Vec<u128>) -> u128 {
    v.sort_unstable();
    v[v.len() / 2]
}

fn runs_for(len: usize) -> usize {
    if len <= 100_000 {
        31
    } else if len <= 1_000_000 {
        15
    } else {
        11
    }
}

/// Median ns of `f` over fresh copies of `init`, and the last result.
fn time<S: Copy>(init: &[S], runs: usize, mut f: impl FnMut(&mut [S])) -> (u128, Vec<S>) {
    let mut buf = init.to_vec();
    for _ in 0..2 {
        buf.copy_from_slice(init);
        f(black_box(&mut buf));
    }
    let mut t = Vec::with_capacity(runs);
    for _ in 0..runs {
        buf.copy_from_slice(init);
        let b = black_box(&mut buf);
        let t0 = Instant::now();
        f(b);
        t.push(t0.elapsed().as_nanos());
    }
    (median(t), buf)
}

fn row<S: Copy + PartialEq>(
    case: &str,
    d: &str,
    len: usize,
    nslots: usize,
    variant: &str,
    ns: u128,
    got: &[S],
    want: &[S],
    bits: impl Fn(S) -> u64,
) {
    let eq = got.iter().zip(want).all(|(&a, &b)| bits(a) == bits(b));
    println!(
        "{case}\t{d}\t{len}\t{nslots}\t{variant}\t{ns}\t{:.1}\t{}",
        len as f64 / ns as f64 * 1e3, // Mitems/s
        if eq { "eq" } else { "DIFF" }
    );
}

fn classes(keys: &[u32], w: usize) -> (f64, f64, f64) {
    let mut c = [0usize; 3];
    for win in keys.chunks_exact(w) {
        c[classify(win) as usize] += 1;
    }
    let t = c.iter().sum::<usize>().max(1) as f64;
    (
        c[Class::Contiguous as usize] as f64 / t,
        c[Class::Distinct as usize] as f64 / t,
        c[Class::Conflict as usize] as f64 / t,
    )
}

fn main() {
    println!(
        "# avx2_static={} fma_static={} cores_pinned_by_caller; columns: case dist items slots variant median_ns Mitems/s eq",
        cfg!(target_feature = "avx2"),
        cfg!(target_feature = "fma")
    );
    assert!(has_avx2());
    let mut r = Rng(20261027);
    if std::env::args().nth(1).as_deref() == Some("knee") {
        // Where the scalar reduce slows down: sum32, 8 uniform keys, sizes around the L2 (512 KB = 64k items).
        for len in [10_000usize, 20_000, 40_000, 60_000, 80_000, 100_000, 200_000] {
            let ks = make_keys(&mut r, len, 8, Dist::Uniform);
            let items: Vec<Xi> = ks.iter().map(|&k| Xi { index: k, x: r.unit() }).collect();
            let init: Vec<f32> = (0..8).map(|_| r.unit()).collect();
            let (ts, rs) = time(&init, 31, |s| sum32_ref(&items, s));
            let (td, rd) = time(&init, 31, |s| sum32_simd(&items, s));
            row("sum32", "uniform", len, 8, "scalar", ts, &rs, &rs, |x| x.to_bits() as u64);
            row("sum32", "uniform", len, 8, "avx2-reg", td, &rd, &rs, |x| x.to_bits() as u64);
        }
        // Same size, different data: is the scalar rate a function of the data?
        for len in [100_000usize, 1_000_000] {
            for seed in 0..12u64 {
                let mut r = Rng(seed);
                let ks = make_keys(&mut r, len, 8, Dist::Uniform);
                let items: Vec<Xi> = ks.iter().map(|&k| Xi { index: k, x: r.unit() }).collect();
                let init: Vec<f32> = (0..8).map(|_| r.unit()).collect();
                let (ts, rs) = time(&init, 15, |s| sum32_ref(&items, s));
                let (td, rd) = time(&init, 15, |s| sum32_simd(&items, s));
                row("sum32-seed", &format!("seed{seed}"), len, 8, "scalar", ts, &rs, &rs, |x| x.to_bits() as u64);
                row("sum32-seed", &format!("seed{seed}"), len, 8, "avx2-reg", td, &rd, &rs, |x| x.to_bits() as u64);
            }
        }
        return;
    }
    let sizes = [1_000usize, 10_000, 100_000, 1_000_000, 10_000_000];

    // Map: each item its own key (identity and a random permutation).
    for &len in &sizes {
        for d in [Dist::Ident, Dist::Perm] {
            let ks = make_keys(&mut r, len, len, d);
            let items: Vec<Xi> = ks.iter().map(|&k| Xi { index: k, x: r.unit() }).collect();
            let init: Vec<f32> = (0..len).map(|_| r.unit()).collect();
            let runs = runs_for(len);
            let cx = Cx { a: 0.37, t: -0.25 };
            let (ts, rs) = time(&init, runs, |s| axpy_ref(&cx, &items, s));
            let (tv, rv) = time(&init, runs, |s| axpy_simd(&cx, &items, s));
            row("axpy", d.name(), len, len, "scalar", ts, &rs, &rs, |x| x.to_bits() as u64);
            row("axpy", d.name(), len, len, "avx2", tv, &rv, &rs, |x| x.to_bits() as u64);
            let (ts, rs) = time(&init, runs, |s| axpy_sel_ref(&cx, &items, s));
            let (tv, rv) = time(&init, runs, |s| axpy_sel_simd(&cx, &items, s));
            row("axpy_sel", d.name(), len, len, "scalar", ts, &rs, &rs, |x| x.to_bits() as u64);
            row("axpy_sel", d.name(), len, len, "avx2", tv, &rv, &rs, |x| x.to_bits() as u64);
            let cp = Cp { c: [0.5, -0.31, 0.12, 0.07, -0.02, 0.011, -0.003, 0.0007] };
            let (ts, rs) = time(&init, runs, |s| poly_ref(&cp, &items, s));
            let (tv, rv) = time(&init, runs, |s| poly_simd(&cp, &items, s));
            row("poly", d.name(), len, len, "scalar", ts, &rs, &rs, |x| x.to_bits() as u64);
            row("poly", d.name(), len, len, "avx2", tv, &rv, &rs, |x| x.to_bits() as u64);
        }
    }

    // Reduce by key: 8, 64 and 65536 keys; uniform and skewed.
    for &len in &sizes {
        for &n in &[8usize, 64, 65536] {
            for d in [Dist::Uniform, Dist::Skew] {
                let ks = make_keys(&mut r, len, n, d);
                let runs = runs_for(len);
                let nm = format!("{}", n);
                {
                    let items: Vec<Xi> = ks.iter().map(|&k| Xi { index: k, x: r.unit() }).collect();
                    let init: Vec<f32> = (0..n).map(|_| r.unit()).collect();
                    let (ts, rs) = time(&init, runs, |s| sum32_ref(&items, s));
                    let (tw, rw) = time(&init, runs, |s| sum32_simd_window(&items, s));
                    let (td, rd) = time(&init, runs, |s| sum32_simd(&items, s));
                    let b = |x: f32| x.to_bits() as u64;
                    row("sum32", d.name(), len, n, "scalar", ts, &rs, &rs, b);
                    row("sum32", d.name(), len, n, "avx2-window", tw, &rw, &rs, b);
                    row("sum32", d.name(), len, n, if n <= 64 { "avx2-reg" } else { "avx2-window" }, td, &rd, &rs, b);
                    if len == 1_000_000 {
                        let (a, b2, c) = classes(&ks, 8);
                        println!(
                            "# windows(8) sum32 n={nm} {}: contiguous {a:.3} distinct {b2:.3} conflict {c:.3}",
                            d.name()
                        );
                    }
                }
                {
                    let items: Vec<Kv64> = ks.iter().map(|&k| Kv64 { index: k, _p: 0, v: r.next() >> 40 }).collect();
                    let init: Vec<u64> = (0..n).map(|_| r.next() >> 40).collect();
                    let (ts, rs) = time(&init, runs, |s| sum64_ref(&items, s));
                    let (tw, rw) = time(&init, runs, |s| sum64_simd_window(&items, s));
                    let (td, rd) = time(&init, runs, |s| sum64_simd(&items, s));
                    let b = |x: u64| x;
                    row("sum64", d.name(), len, n, "scalar", ts, &rs, &rs, b);
                    row("sum64", d.name(), len, n, "avx2-window", tw, &rw, &rs, b);
                    row("sum64", d.name(), len, n, if n <= 16 { "avx2-reg" } else { "avx2-window" }, td, &rd, &rs, b);
                    if len == 1_000_000 {
                        let (a, b2, c) = classes(&ks, 4);
                        println!(
                            "# windows(4) sum64 n={nm} {}: contiguous {a:.3} distinct {b2:.3} conflict {c:.3}",
                            d.name()
                        );
                    }
                }
            }
        }
    }
}
