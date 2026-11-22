//! Tests of the lane versions against the scalar functions, bit for bit. A NaN equals any NaN (the
//! scalar returns `f64::NAN` for every NaN it makes, and an input NaN comes back with its own payload).
//! Each function gets 1_000_000 random bit patterns, 1_000_000 values in its fast-path domain, and the
//! special values with their neighbours within 4 ulps. Every slice is also checked at an odd offset (not
//! 32-byte aligned) and with lengths that are not multiples of 4.
//!
//! The teeth test: `_mm256_max_pd` in place of the scalar's rule must be caught on signed zeros.
//! sin and cos (the small-argument domain and the reduction through the scalar) are in `trig_inputs`.

extern crate std;

use super::avx2::*;
use crate::math::f64 as s;
use core::arch::x86_64::*;
use std::vec::Vec;

const N: usize = 1_000_000;

/// xorshift64*: the same sequence on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }

    /// A value with a uniform random sign, mantissa, and binary exponent in `[lo, hi]`.
    fn log_uniform(&mut self, lo: i64, hi: i64) -> f64 {
        let e = lo + (self.next() % (hi - lo + 1) as u64) as i64;
        let frac = self.next() & ((1 << 52) - 1);
        let sign = self.next() & (1 << 63);
        f64::from_bits(sign | (((e + 1023) as u64) << 52) | frac)
    }
}

fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

/// The special values, and each one's neighbours up to 4 ulps away on both sides.
fn specials() -> Vec<f64> {
    let base = [
        0.0,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::from_bits(0xfff8_0000_0000_0000),
        f64::from_bits(0x7ff0_0000_0000_0001), // signalling NaN
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::MIN_POSITIVE / 2.0,
        -f64::MIN_POSITIVE / 2.0,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MAX,
        -f64::MAX,
        1.0,
        -1.0,
        0.5,
        -0.5,
        2.0,
        1.5,
        -1.5,
        2.5,
        3.5,
        -2.5,
        0.346_573_590_279_972_6, // ln 2 / 2: exp's k = 0 edge
        -0.346_573_590_279_972_6,
        1.0e-300,
        -1.0e-300,
        1.0e300,
        -1.0e300,
        2.0_f64.powi(-28), // exp's fast-path lower edge
        2.0_f64.powi(-29),
        2.0_f64.powi(-27),
        -(2.0_f64.powi(-28)),
        709.78,
        709.782_712_893_384, // O_THRESHOLD
        709.782_226_562_5,   // high word 0x40862e42, the branch threshold
        -708.39,
        -708.4,
        -745.133_219_101_941_1,
        -745.0,
        1024.0,
        2.0_f64.powi(1023),
        2.0_f64.powi(-1022),
        2.0_f64.powi(52),
        2.0_f64.powi(53),
        4.5e15,
        std::f64::consts::PI,
        std::f64::consts::FRAC_PI_2,
        1.0e6,
        -1.0e6,
    ];
    let mut out = Vec::new();
    for &v in &base {
        let mut w = v;
        out.push(w);
        for _ in 0..4 {
            w = next_up(w);
            out.push(w);
        }
        let mut w = v;
        for _ in 0..4 {
            w = next_down(w);
            out.push(w);
        }
    }
    out
}

fn next_up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let b = x.to_bits();
    f64::from_bits(if x > 0.0 { b + 1 } else { b - 1 })
}

fn next_down(x: f64) -> f64 {
    -next_up(-x)
}

/// The mismatches of `lanes` against `scalar` on `xs`, at the offsets and lengths the module promises.
/// Panics on the first mismatch, with its input and both outputs.
fn check_unary(name: &str, lanes: fn(&[f64], &mut [f64]), scalar: fn(f64) -> f64, xs: &[f64]) {
    for off in [0_usize, 1, 3] {
        let input = &xs[off..];
        let mut buf = std::vec![0.0_f64; input.len() + 1];
        let out = &mut buf[1..];
        lanes(input, out);
        for (i, (&x, &o)) in input.iter().zip(out.iter()).enumerate() {
            let r = scalar(x);
            assert!(
                same(o, r),
                "{name}: input {x:e} (bits {:#018x}) at index {i} (offset {off}): lanes {o:e} ({:#018x}), scalar {r:e} ({:#018x})",
                x.to_bits(),
                o.to_bits(),
                r.to_bits()
            );
        }
    }
}

fn check_binary(name: &str, lanes: fn(&[f64], &[f64], &mut [f64]), scalar: fn(f64, f64) -> f64, a: &[f64], b: &[f64]) {
    for off in [0_usize, 1, 3] {
        let (a, b) = (&a[off..], &b[off..]);
        let mut buf = std::vec![0.0_f64; a.len() + 1];
        let out = &mut buf[1..];
        lanes(a, b, out);
        for i in 0..a.len() {
            let r = scalar(a[i], b[i]);
            assert!(
                same(out[i], r),
                "{name}: inputs ({:e}, {:e}) at index {i} (offset {off}): lanes {:e}, scalar {:e}",
                a[i],
                b[i],
                out[i],
                r
            );
        }
    }
}

fn check_ternary(
    name: &str,
    lanes: fn(&[f64], &[f64], &[f64], &mut [f64]),
    scalar: fn(f64, f64, f64) -> f64,
    a: &[f64],
    b: &[f64],
    c: &[f64],
) {
    for off in [0_usize, 1, 3] {
        let (a, b, c) = (&a[off..], &b[off..], &c[off..]);
        let mut buf = std::vec![0.0_f64; a.len() + 1];
        let out = &mut buf[1..];
        lanes(a, b, c, out);
        for i in 0..a.len() {
            let r = scalar(a[i], b[i], c[i]);
            assert!(
                same(out[i], r),
                "{name}: inputs ({:e}, {:e}, {:e}) at index {i} (offset {off}): lanes {:e}, scalar {:e}",
                a[i],
                b[i],
                c[i],
                out[i],
                r
            );
        }
    }
}

/// The unary inputs: random bit patterns, values in `[lo, hi]`, log-uniform magnitudes, and specials.
fn unary_inputs(seed: u64, lo: f64, hi: f64, log_lo: i64, log_hi: i64) -> Vec<f64> {
    let mut rng = Rng(seed);
    let mut xs = Vec::with_capacity(3 * N + 500);
    for _ in 0..N {
        xs.push(f64::from_bits(rng.next()));
    }
    for _ in 0..N {
        xs.push(lo + (hi - lo) * rng.unit());
    }
    for _ in 0..N {
        xs.push(rng.log_uniform(log_lo, log_hi));
    }
    xs.extend(specials());
    xs
}

fn binary_inputs(seed: u64) -> (Vec<f64>, Vec<f64>) {
    let mut rng = Rng(seed);
    let mut a = Vec::new();
    let mut b = Vec::new();
    for _ in 0..N {
        a.push(f64::from_bits(rng.next()));
        b.push(f64::from_bits(rng.next()));
    }
    let sp = specials();
    for &x in &sp {
        for &y in &sp {
            a.push(x);
            b.push(y);
        }
    }
    (a, b)
}

fn require_avx2() -> bool {
    if has_avx2_fma() {
        true
    } else {
        std::eprintln!("skipped: this CPU has no AVX2+FMA; the lane code is not run");
        false
    }
}

#[test]
fn exp_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let xs = unary_inputs(0x1111, -745.0, 710.0, -40, 10);
    check_unary("exp", exp_slice, s::exp, &xs);
}

#[test]
fn ln_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let xs = unary_inputs(0x2222, 1.0e-300, 1.0e300, -1074, 1023);
    check_unary("ln", ln_slice, s::ln, &xs);
}

#[test]
fn sqrt_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let xs = unary_inputs(0x3333, -1.0e6, 1.0e6, -1074, 1023);
    check_unary("sqrt", sqrt_slice, s::sqrt, &xs);
}

#[test]
fn floor_ceil_trunc_round_match_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let xs = unary_inputs(0x4444, -1.0e6, 1.0e6, -1074, 1023);
    check_unary("floor", floor_slice, s::floor, &xs);
    check_unary("ceil", ceil_slice, s::ceil, &xs);
    check_unary("trunc", trunc_slice, s::trunc, &xs);
    check_unary("round_ties_even", round_ties_even_slice, s::round_ties_even, &xs);
}

#[test]
fn max_min_match_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let (a, b) = binary_inputs(0x5555);
    check_binary("max", max_slice, s::max, &a, &b);
    check_binary("min", min_slice, s::min, &a, &b);
}

#[test]
fn mul_add_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let mut rng = Rng(0x6666);
    let mut a = Vec::new();
    let mut b = Vec::new();
    let mut c = Vec::new();
    for _ in 0..N {
        // Mix of random patterns and values with a moderate exponent, so the sum is near the product.
        a.push(f64::from_bits(rng.next()));
        b.push(rng.log_uniform(-600, 600));
        c.push(f64::from_bits(rng.next()));
        a.push(rng.log_uniform(-300, 300));
        b.push(rng.log_uniform(-300, 300));
        c.push(-(a[a.len() - 1] * b[b.len() - 1]) * (1.0 + 1.0e-16 * rng.unit()));
    }
    let sp = specials();
    for &x in &sp {
        for &y in sp.iter().step_by(3) {
            for &z in sp.iter().step_by(7) {
                a.push(x);
                b.push(y);
                c.push(z);
            }
        }
    }
    check_ternary("mul_add", mul_add_slice, s::mul_add, &a, &b, &c);
}

/// Each lane function gets an input vector of four lanes from the domain: the fast path must be taken,
/// that is, the guard must pass, and the result must match the scalar. The guard is the whole claim.
#[test]
fn fast_path_is_taken_on_domain() {
    if !require_avx2() {
        return;
    }
    let mut rng = Rng(0x7777);
    let mut taken = 0_usize;
    let groups = N / 4;
    for _ in 0..groups {
        let mut v = [0.0_f64; 4];
        for x in &mut v {
            *x = -700.0 + 1405.0 * rng.unit(); // exp's domain, and ln's uses are below
        }
        // SAFETY: has_avx2_fma() held in require_avx2().
        let in_dom = unsafe { exp_in_domain(_mm256_loadu_pd(v.as_ptr())) };
        if in_dom {
            taken += 1;
        }
        let mut w = [0.0_f64; 4];
        for x in &mut w {
            *x = 1.0e-300 + 1.0e300 * rng.unit();
        }
        // SAFETY: as above.
        let ln_dom = unsafe { ln_in_domain(_mm256_loadu_pd(w.as_ptr())) };
        assert!(ln_dom, "ln guard rejected a positive normal group {w:?}");
    }
    // Exp's domain excludes |x| >= 709.78 and |x| < 2^-28: a few groups of 4 random values fail. The
    // fast path must still be the common case.
    assert!(taken * 10 > groups * 9, "exp fast path taken on only {taken} of {groups} groups");
}

/// The teeth: `_mm256_max_pd` is not the scalar's max. On signed zeros and NaN it must disagree, so the
/// bitwise check above would catch a lane max written that way. The real `max_f64x4` must agree.
#[test]
fn broken_max_is_caught_on_signed_zeros() {
    if !require_avx2() {
        return;
    }
    // SAFETY: AVX2 holds (require_avx2).
    #[target_feature(enable = "avx2")]
    unsafe fn broken_max(a: __m256d, b: __m256d) -> __m256d {
        _mm256_max_pd(a, b)
    }
    // SAFETY: AVX2 holds (require_avx2).
    #[target_feature(enable = "avx2")]
    unsafe fn lane_mismatches(f: unsafe fn(__m256d, __m256d) -> __m256d, a: &[f64], b: &[f64]) -> usize {
        let mut bad = 0;
        for i in (0..a.len()).step_by(4) {
            let mut r = [0.0_f64; 4];
            // SAFETY: the loop runs over whole groups of 4 (the inputs are multiples of 4 in length).
            unsafe {
                _mm256_storeu_pd(r.as_mut_ptr(), f(_mm256_loadu_pd(a.as_ptr().add(i)), _mm256_loadu_pd(b.as_ptr().add(i))));
            }
            for j in 0..4 {
                if !same(r[j], s::max(a[i + j], b[i + j])) {
                    bad += 1;
                }
            }
        }
        bad
    }
    let vals = [0.0_f64, -0.0, 1.0, -1.0, f64::NAN, f64::INFINITY];
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for &x in &vals {
        for &y in &vals {
            a.push(x);
            b.push(y);
        }
    }
    while a.len() % 4 != 0 {
        a.push(0.0);
        b.push(0.0);
    }
    // SAFETY: as above.
    let broken = unsafe { lane_mismatches(broken_max, &a, &b) };
    // SAFETY: as above.
    let real = unsafe { lane_mismatches(max_f64x4, &a, &b) };
    assert!(broken > 0, "the broken lane max was not caught: the check has no teeth");
    assert_eq!(real, 0, "the real lane max disagrees with the scalar");
}

/// Sanity of the detection: the probe agrees with what the cpu must have when the tests run here.
#[test]
fn detection_is_stable() {
    let first = has_avx2_fma();
    for _ in 0..3 {
        assert_eq!(has_avx2_fma(), first);
    }
}

/// The trig inputs: the small-argument domain `[-pi/4, pi/4]` (the fast path), random bit patterns and
/// log-uniform magnitudes (mostly the scalar reduction), and the multiples of pi/2 with their ulp
/// neighbours, the pi/4 boundary, subnormals, zeros, infinities and NaN.
fn trig_inputs(seed: u64) -> Vec<f64> {
    let pio4 = std::f64::consts::FRAC_PI_4;
    let mut xs = unary_inputs(seed, -pio4, pio4, -1074, -1);
    let mut rng = Rng(seed ^ 0x9e37_79b9);
    // Multiples of pi/2 near the reduction's ranges, with the neighbours at 1 ulp and 2 ulps.
    for _ in 0..N {
        let k = (rng.next() % (1 << 20)) as f64 + rng.unit();
        let base = k * std::f64::consts::FRAC_PI_2;
        let sign = if rng.next() & 1 == 0 { 1.0 } else { -1.0 };
        let v = sign * base;
        xs.push(v);
        xs.push(next_up(v));
        xs.push(next_down(v));
    }
    for k in 0..=64_i32 {
        let v = f64::from(k) * std::f64::consts::FRAC_PI_2;
        for w in [v, -v] {
            xs.push(w);
            xs.push(next_up(w));
            xs.push(next_down(w));
        }
    }
    // The pi/4 boundary of the fast path, `hi(x) = 0x3fe921fb`, and its neighbours.
    let edge = f64::from_bits(0x3fe9_21fb_5444_2d18);
    let mut w = edge;
    for _ in 0..8 {
        xs.push(w);
        xs.push(-w);
        w = next_up(w);
    }
    let mut w = edge;
    for _ in 0..8 {
        w = next_down(w);
        xs.push(w);
        xs.push(-w);
    }
    // Subnormals and the tiny-|x| branch (|x| < 2^-27), in both signs.
    for _ in 0..N / 8 {
        let frac = rng.next() & ((1 << 52) - 1);
        let v = f64::from_bits(frac); // subnormal (or 0)
        xs.push(v);
        xs.push(-v);
    }
    xs.extend(specials());
    xs
}

/// The fast path of `sin` and `cos` is taken on `[-pi/4, pi/4]`: the guard passes for every group of four
/// in the domain, and rejects one with `pi/2` in it.
#[test]
fn trig_fast_path_is_taken_on_domain() {
    if !require_avx2() {
        return;
    }
    let pio4 = std::f64::consts::FRAC_PI_4;
    let mut rng = Rng(0x8888);
    for _ in 0..N / 4 {
        let mut v = [0.0_f64; 4];
        for x in &mut v {
            *x = -pio4 + 2.0 * pio4 * rng.unit();
        }
        // SAFETY: has_avx2_fma() held in require_avx2().
        let ok = unsafe { trig_small_in_domain(_mm256_loadu_pd(v.as_ptr())) };
        assert!(ok, "sin/cos guard rejected an in-domain group {v:?}");
    }
    // SAFETY: as above.
    let out = unsafe { trig_small_in_domain(_mm256_set_pd(1.0, 1.0, std::f64::consts::FRAC_PI_2, 1.0)) };
    assert!(!out, "sin/cos guard accepted pi/2, which the reduction must handle");
}

#[test]
fn sin_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let xs = trig_inputs(0x1234);
    check_unary("sin", sin_slice, s::sin, &xs);
}

#[test]
fn cos_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let xs = trig_inputs(0x5678);
    check_unary("cos", cos_slice, s::cos, &xs);
}

/// The teeth for sin: a lane sin that drops the last Horner term of the polynomial, the kind of edit a
/// reordering could make, must disagree with the scalar on the domain. The real lane sin must agree.
#[test]
fn broken_sin_is_caught() {
    if !require_avx2() {
        return;
    }
    // SAFETY: AVX2 holds (require_avx2). Broken: S6 dropped from the polynomial.
    #[target_feature(enable = "avx2")]
    unsafe fn broken_sin(x: __m256d) -> __m256d {
        {
            let z = _mm256_mul_pd(x, x);
            let v = _mm256_mul_pd(z, x);
            let c = |k: f64| _mm256_set1_pd(k);
            let r = _mm256_add_pd(
                c(8.33333333332248946124e-03),
                _mm256_mul_pd(
                    z,
                    _mm256_add_pd(c(-1.98412698298579493134e-04), _mm256_mul_pd(z, c(2.75573137070700676789e-06))),
                ),
            );
            _mm256_add_pd(x, _mm256_mul_pd(v, _mm256_add_pd(c(-1.66666666666666324348e-01), _mm256_mul_pd(z, r))))
        }
    }
    // SAFETY: AVX2 holds (require_avx2).
    #[target_feature(enable = "avx2")]
    unsafe fn lane_mismatches(f: unsafe fn(__m256d) -> __m256d, xs: &[f64]) -> usize {
        let mut bad = 0;
        for c in xs.chunks_exact(4) {
            let mut r = [0.0_f64; 4];
            // SAFETY: the chunk has four elements.
            unsafe { _mm256_storeu_pd(r.as_mut_ptr(), f(_mm256_loadu_pd(c.as_ptr()))) };
            for j in 0..4 {
                if !same(r[j], s::sin(c[j])) {
                    bad += 1;
                }
            }
        }
        bad
    }
    let pio4 = std::f64::consts::FRAC_PI_4;
    let mut rng = Rng(0x9999);
    let xs: Vec<f64> = (0..4 * 4096).map(|_| -pio4 + 2.0 * pio4 * rng.unit()).collect();
    // SAFETY: as above.
    let broken = unsafe { lane_mismatches(broken_sin, &xs) };
    // SAFETY: as above.
    let real = unsafe { lane_mismatches(sin_f64x4, &xs) };
    assert!(broken > 0, "the broken lane sin was not caught: the check has no teeth");
    assert_eq!(real, 0, "the real lane sin disagrees with the scalar");
}

/// The bench's exp input domain, `uniform[-700, 700]` (check/math/bench.rs): every group of four must
/// pass exp's guard, so no vector goes to the scalar, and the bits must match the scalar. The denormal
/// penalty of the discarded branch (see `exp_fast`) showed up here as a 5x slowdown, not as a bit change,
/// so the timing is the check that catches it; this test keeps the guard claim and the bits.
#[test]
fn exp_bench_domain_takes_fast_path_and_matches() {
    if !require_avx2() {
        return;
    }
    let mut rng = Rng(0x5151);
    let xs: Vec<f64> = (0..N).map(|_| -700.0 + 1400.0 * rng.unit()).collect();
    for c in xs.chunks_exact(4) {
        // SAFETY: has_avx2_fma() held in require_avx2().
        assert!(unsafe { exp_in_domain(_mm256_loadu_pd(c.as_ptr())) }, "guard sent {c:?} to the scalar");
    }
    check_unary("exp", exp_slice, s::exp, &xs);
}

#[test]
fn exp_m1_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    // Exponents -60..6 cover 2^-54 (the tiny early return) and 38.8 (the large-|x| returns).
    let xs = unary_inputs(0x3333, -45.0, 40.0, -60, 6);
    check_unary("exp_m1", exp_m1_slice, s::exp_m1, &xs);
}

/// Every branch of exp_m1's reduction and of its k-dispatch is taken on the domain: the guard passes on
/// all of these groups, and the lane output matches the scalar on each k class (k = 0, +-1, 2..19, 20..56,
/// and the negative k <= -2), so no class is only exercised by the specials.
#[test]
fn exp_m1_fast_path_taken_and_all_k_classes_match() {
    if !require_avx2() {
        return;
    }
    let mut rng = Rng(0x4444);
    let mut xs: Vec<f64> = Vec::with_capacity(4 * N);
    for _ in 0..N {
        xs.push(-38.0 + 76.0 * rng.unit());
    }
    for _ in 0..N {
        xs.push(0.35 * rng.unit() * if rng.next() & 1 == 0 { 1.0 } else { -1.0 });
    }
    for _ in 0..N {
        xs.push(1.0e-16 * (1.0 + rng.unit())); // above 2^-54, below 2^-53: the smallest in the domain
    }
    for _ in 0..N {
        xs.push(1.0 + 37.0 * rng.unit());
    }
    for c in xs.chunks_exact(4) {
        // SAFETY: has_avx2_fma() held in require_avx2().
        assert!(unsafe { exp_m1_in_domain(_mm256_loadu_pd(c.as_ptr())) }, "guard rejected a domain group {c:?}");
    }
    check_unary("exp_m1", exp_m1_slice, s::exp_m1, &xs);
}

/// The teeth for exp_m1. The comparison must see a one-ulp difference in the k = 0 class and in the k = -1
/// class: a lane that differs by the last bit of its result is caught by `lane_mismatches`. The real lane
/// function must agree. (This tests the comparison on the exp_m1 domain; it does not test a broken
/// reduction, which would need a second copy of exp_m1_fast.)
#[test]
fn one_ulp_exp_m1_is_caught() {
    if !require_avx2() {
        return;
    }
    // SAFETY: AVX2 holds (require_avx2). Broken: the lane result with its last bit flipped.
    #[target_feature(enable = "avx2,fma")]
    unsafe fn broken_exp_m1(x: __m256d) -> __m256d {
        unsafe {
            let r = exp_m1_f64x4(x);
            _mm256_castsi256_pd(_mm256_xor_si256(_mm256_castpd_si256(r), _mm256_set1_epi64x(1)))
        }
    }
    // SAFETY: as above.
    #[target_feature(enable = "avx2,fma")]
    unsafe fn lane_mismatches(f: unsafe fn(__m256d) -> __m256d, xs: &[f64]) -> usize {
        let mut bad = 0;
        for c in xs.chunks_exact(4) {
            let mut r = [0.0_f64; 4];
            // SAFETY: the chunk has four elements.
            unsafe { _mm256_storeu_pd(r.as_mut_ptr(), f(_mm256_loadu_pd(c.as_ptr()))) };
            for j in 0..4 {
                if !same(r[j], s::exp_m1(c[j])) {
                    bad += 1;
                }
            }
        }
        bad
    }
    let mut rng = Rng(0x5555);
    let xs: Vec<f64> = (0..4 * 4096).map(|_| -38.0 + 76.0 * rng.unit()).collect();
    // SAFETY: as above.
    let broken = unsafe { lane_mismatches(broken_exp_m1, &xs) };
    // SAFETY: as above.
    let real = unsafe { lane_mismatches(exp_m1_f64x4, &xs) };
    assert!(broken > 0, "the one-ulp change was not caught: the check has no teeth");
    assert_eq!(real, 0, "the real lane exp_m1 disagrees with the scalar");
}

#[test]
fn log2_matches_scalar_bitwise() {
    if !require_avx2() {
        return;
    }
    let xs = unary_inputs(0x6666, 1.0e-300, 1.0e300, -1000, 1000);
    check_unary("log2", log2_slice, s::log2, &xs);
}

/// The guard passes on the positive normal numbers, including those next to 1.0, and fails on 1.0 itself
/// (the scalar's early return), so the fast path covers the whole domain except that one value.
#[test]
fn log2_fast_path_taken_on_domain() {
    if !require_avx2() {
        return;
    }
    let mut rng = Rng(0x7070);
    let mut xs: Vec<f64> = Vec::with_capacity(4 * N);
    for _ in 0..N {
        xs.push(1.0 + 1.0e-12 * (1.0 + rng.unit())); // strictly above 1.0 (1.0 + tiny can round to 1.0)
    }
    for _ in 0..N {
        xs.push(f64::MIN_POSITIVE * (1.0 + rng.unit()) * 2.0_f64.powi(rng.next() as i32 & 0x3ff));
    }
    for c in xs.chunks_exact(4) {
        // SAFETY: has_avx2_fma() held in require_avx2().
        assert!(unsafe { log2_in_domain(_mm256_loadu_pd(c.as_ptr())) }, "guard rejected a domain group {c:?}");
    }
    check_unary("log2", log2_slice, s::log2, &xs);
    // SAFETY: as above.
    assert!(!unsafe { log2_in_domain(_mm256_set1_pd(1.0)) }, "1.0 must go to the scalar");
}

/// The teeth for log2: the last bit of the lane result flipped must be caught, and the real lane log2
/// must agree. (As for exp_m1, this tests the comparison, not a broken reduction.)
#[test]
fn one_ulp_log2_is_caught() {
    if !require_avx2() {
        return;
    }
    // SAFETY: AVX2 holds (require_avx2). Broken: the lane result with its last bit flipped.
    #[target_feature(enable = "avx2,fma")]
    unsafe fn broken_log2(x: __m256d) -> __m256d {
        unsafe {
            let r = log2_f64x4(x);
            _mm256_castsi256_pd(_mm256_xor_si256(_mm256_castpd_si256(r), _mm256_set1_epi64x(1)))
        }
    }
    // SAFETY: as above.
    #[target_feature(enable = "avx2,fma")]
    unsafe fn lane_mismatches(f: unsafe fn(__m256d) -> __m256d, xs: &[f64]) -> usize {
        let mut bad = 0;
        for c in xs.chunks_exact(4) {
            let mut r = [0.0_f64; 4];
            // SAFETY: the chunk has four elements.
            unsafe { _mm256_storeu_pd(r.as_mut_ptr(), f(_mm256_loadu_pd(c.as_ptr()))) };
            for j in 0..4 {
                if !same(r[j], s::log2(c[j])) {
                    bad += 1;
                }
            }
        }
        bad
    }
    let mut rng = Rng(0x8080);
    let xs: Vec<f64> = (0..4 * 4096).map(|_| 1.0e-3 + 1.0e6 * rng.unit()).collect();
    // SAFETY: as above.
    let broken = unsafe { lane_mismatches(broken_log2, &xs) };
    // SAFETY: as above.
    let real = unsafe { lane_mismatches(log2_f64x4, &xs) };
    assert!(broken > 0, "the one-ulp change was not caught: the check has no teeth");
    assert_eq!(real, 0, "the real lane log2 disagrees with the scalar");
}
