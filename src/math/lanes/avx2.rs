//! AVX2 + FMA lane versions of `trame::math::f64` (x86-64-v3): four f64 lanes per vector.
//!
//! Preconditions. Every `unsafe fn` here needs the CPU to have AVX2 and FMA, which its `target_feature`
//! states. The safe slice functions check it with `has_avx2_fma()`, once. The crate is `no_std`, so that
//! check reads cpuid and xgetbv from `core::arch`, as `is_x86_feature_detected!` does, instead of calling it.
//!
//! Exactness. A lane function does, in each lane, the operations of the scalar function, in its order:
//! * one lane operation per scalar operation, same operands, IEEE round to nearest, no reassociation;
//! * `fmadd` only in `mul_add`, which the scalar computes fused too (src/math/f64/fma.rs);
//! * a scalar branch is a select: both sides are computed and the lane keeps one. A discarded side may
//!   produce inf or NaN, which does not trap in Rust;
//! * a scalar branch that is not a select (special values, extreme arguments, where the scalar returns
//!   early) is a guard. If any lane of a vector fails the guard, the whole vector is recomputed by the
//!   scalar function, lane by lane. That is exact by construction.
//! * every NaN the scalar returns is `f64::NAN`; `canon` makes the lanes return the same bits.
//!
//! The constants of exp and ln are copied from src/math/f64/{exp,ln}.rs, whose copies are private. The
//! tests in `super::tests` compare bits with the scalar functions, which pins them.

use crate::math::f64 as s;
use core::arch::x86_64::*;
use core::sync::atomic::{AtomicU8, Ordering};

// ---------------------------------------------------------------------------------------------
// Detection and the scalar fallback

/// Whether the CPU runs AVX2 and FMA, with the OS saving the YMM state. Probed once and cached.
pub fn has_avx2_fma() -> bool {
    static STATE: AtomicU8 = AtomicU8::new(0); // 0 unknown, 1 yes, 2 no
    match STATE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let ok = probe();
            STATE.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            ok
        }
    }
}

fn probe() -> bool {
    // SAFETY: cpuid exists on every x86-64 CPU; leaf 7 is read only if leaf 0 reports it.
    unsafe {
        if __cpuid(0).eax < 7 {
            return false;
        }
        let l1 = __cpuid(1);
        let l7 = __cpuid_count(7, 0);
        let osxsave = l1.ecx & (1 << 27) != 0;
        let avx = l1.ecx & (1 << 28) != 0;
        let fma = l1.ecx & (1 << 12) != 0;
        let avx2 = l7.ebx & (1 << 5) != 0;
        if !(osxsave && avx && fma && avx2) {
            return false;
        }
        // SAFETY: OSXSAVE is set, so xgetbv is legal.
        (xgetbv0() & 0b110) == 0b110
    }
}

#[target_feature(enable = "xsave")]
unsafe fn xgetbv0() -> u64 {
    // SAFETY: the caller checked OSXSAVE.
    unsafe { _xgetbv(0) }
}

/// The scalar function on each lane. Used only for vectors that fail a guard.
#[target_feature(enable = "avx2")]
#[inline(never)]
unsafe fn scalar4(x: __m256d, f: fn(f64) -> f64) -> __m256d {
    unsafe {
        let mut a = [0.0_f64; 4];
        _mm256_storeu_pd(a.as_mut_ptr(), x);
        for v in &mut a {
            *v = f(*v);
        }
        _mm256_loadu_pd(a.as_ptr())
    }
}

// ---------------------------------------------------------------------------------------------
// Lane helpers

/// `m ? t : f` per lane, for a float mask (a lane is true when its sign bit is set; compares give all ones).
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn sel(m: __m256d, t: __m256d, f: __m256d) -> __m256d {
    _mm256_blendv_pd(f, t, m)
}

/// `m ? t : f` per lane, for an integer mask (all ones or all zeros per lane).
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn seli(m: __m256i, t: __m256i, f: __m256i) -> __m256i {
    _mm256_blendv_epi8(f, t, m)
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn pd(x: f64) -> __m256d {
    _mm256_set1_pd(x)
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn ei(x: i64) -> __m256i {
    _mm256_set1_epi64x(x)
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn bits(x: __m256d) -> __m256i {
    _mm256_castpd_si256(x)
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn fbits(x: __m256i) -> __m256d {
    _mm256_castsi256_pd(x)
}

/// A lane's integer `k` as f64, exact for `|k| < 2^51`. The bits `0x4330...` or `(k + 2^51)` are the
/// double `2^52 + (k + 2^51)`; subtracting `2^52 + 2^51` leaves `k`. AVX2 has no int64-to-f64 convert.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn i64_to_f64(k: __m256i) -> __m256d {
    unsafe {
        let m = _mm256_add_epi64(k, ei(1_i64 << 51));
        let b = _mm256_or_si256(m, ei(0x4330_0000_0000_0000));
        _mm256_sub_pd(fbits(b), pd(6_755_399_441_055_744.0)) // 2^52 + 2^51
    }
}

/// `with_hi(x, (hi(x) + addend) mod 2^32)`, the scalar's `hi(y).wrapping_add((k << 20) as u32)`. `addend`
/// is a signed 64-bit lane; only its low 32 bits count, as in the scalar's `as u32`.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn add_to_hi(x: __m256d, addend: __m256i) -> __m256d {
    unsafe {
        let b = bits(x);
        let m32 = ei(0xffff_ffff);
        let hi = _mm256_srli_epi64::<32>(b);
        let new_hi = _mm256_and_si256(_mm256_add_epi64(hi, addend), m32);
        let lo = _mm256_and_si256(b, m32);
        fbits(_mm256_or_si256(_mm256_slli_epi64::<32>(new_hi), lo))
    }
}

/// The NaN the scalar functions return, `f64::NAN` (positive quiet). NaN lanes take it.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn canon(r: __m256d) -> __m256d {
    unsafe {
        let is_nan = _mm256_cmp_pd::<_CMP_UNORD_Q>(r, r);
        sel(is_nan, pd(f64::NAN), r)
    }
}

// ---------------------------------------------------------------------------------------------
// Exact functions: the lane instruction is the scalar function bit for bit (see each scalar's file).

/// `sqrt` (src/math/f64/sqrt.rs). `vsqrtpd` is correctly rounded, so it equals the scalar on every input
/// except negatives: the scalar returns `f64::NAN` there, `vsqrtpd` the default NaN (sign set). That is
/// fixed by a select. A NaN input comes back as itself, as in the scalar.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn sqrt_f64x4(x: __m256d) -> __m256d {
    unsafe {
        let r = _mm256_sqrt_pd(x);
        let neg = _mm256_cmp_pd::<_CMP_LT_OQ>(x, pd(0.0));
        sel(neg, pd(f64::NAN), r)
    }
}

/// `floor` (src/math/f64/floor.rs): toward -inf. `floor(-0.0)` is `-0.0`; NaN and infinities unchanged.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn floor_f64x4(x: __m256d) -> __m256d {
    _mm256_round_pd::<{ _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }>(x)
}

/// `ceil` (floor.rs): toward +inf. `ceil(-0.3)` is `-0.0`.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn ceil_f64x4(x: __m256d) -> __m256d {
    _mm256_round_pd::<{ _MM_FROUND_TO_POS_INF | _MM_FROUND_NO_EXC }>(x)
}

/// `trunc` (floor.rs): toward zero. `trunc(-0.3)` is `-0.0`.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn trunc_f64x4(x: __m256d) -> __m256d {
    _mm256_round_pd::<{ _MM_FROUND_TO_ZERO | _MM_FROUND_NO_EXC }>(x)
}

/// `round_ties_even` (floor.rs): nearest, ties to even; a zero result keeps the sign of `x`, as the
/// scalar's `copysign` does, and as `vroundpd` does.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn round_ties_even_f64x4(x: __m256d) -> __m256d {
    _mm256_round_pd::<{ _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC }>(x)
}

/// `max` (src/math/f64/minmax.rs), lane by lane. Not `_mm256_max_pd`: that returns its second operand
/// on NaN and on equal zeros, where the scalar's NaN rule and zero-sign rule differ.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn max_f64x4(a: __m256d, b: __m256d) -> __m256d {
    unsafe {
        let a_nan = _mm256_cmp_pd::<_CMP_UNORD_Q>(a, a);
        let b_nan = _mm256_cmp_pd::<_CMP_UNORD_Q>(b, b);
        let gt = _mm256_cmp_pd::<_CMP_GT_OQ>(a, b);
        let lt = _mm256_cmp_pd::<_CMP_GT_OQ>(b, a);
        let b_neg = fbits(_mm256_cmpgt_epi64(ei(0), bits(b)));
        let eq = sel(b_neg, a, b); // equal: b negative keeps a, else b
        let main = sel(gt, a, sel(lt, b, eq));
        let t = sel(b_nan, a, main); // b is NaN: a
        sel(a_nan, b, t) // a is NaN: b, whatever b is
    }
}

/// `min` (minmax.rs), lane by lane.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn min_f64x4(a: __m256d, b: __m256d) -> __m256d {
    unsafe {
        let a_nan = _mm256_cmp_pd::<_CMP_UNORD_Q>(a, a);
        let b_nan = _mm256_cmp_pd::<_CMP_UNORD_Q>(b, b);
        let lt = _mm256_cmp_pd::<_CMP_LT_OQ>(a, b);
        let gt = _mm256_cmp_pd::<_CMP_LT_OQ>(b, a);
        let a_neg = fbits(_mm256_cmpgt_epi64(ei(0), bits(a)));
        let eq = sel(a_neg, a, b); // equal: a negative keeps a, else b
        let main = sel(lt, a, sel(gt, b, eq));
        let t = sel(b_nan, a, main);
        sel(a_nan, b, t)
    }
}

/// `mul_add` (src/math/f64/fma.rs): `vfmadd` rounds once, as the scalar does. Its NaN results are
/// replaced by `f64::NAN`, as the scalar returns.
///
/// # Safety
/// The CPU has AVX2 and FMA.
#[target_feature(enable = "avx2,fma")]
#[inline]
pub unsafe fn mul_add_f64x4(a: __m256d, b: __m256d, c: __m256d) -> __m256d {
    unsafe { canon(_mm256_fmadd_pd(a, b, c)) }
}

// ---------------------------------------------------------------------------------------------
// exp (src/math/f64/exp.rs, fdlibm e_exp.c)

const EXP_LN2HI: [f64; 2] = [6.93147180369123816490e-01, -6.93147180369123816490e-01];
const EXP_LN2LO: [f64; 2] = [1.90821492927058770002e-10, -1.90821492927058770002e-10];
const EXP_INVLN2: f64 = 1.44269504088896338700e+00;
const EXP_P1: f64 = 1.66666666666666019037e-01;
const EXP_P2: f64 = -2.77777777770155933842e-03;
const EXP_P3: f64 = 6.61375632143793436117e-05;
const EXP_P4: f64 = -1.65339022054652515390e-06;
const EXP_P5: f64 = 4.13813679705723846039e-08;
const EXP_TWOM1000: f64 = 9.33263618503218878990e-302;

/// The fast path of `exp` holds for `2^-28 <= |x| < 709.78`: the high word of `|x|` lies in
/// `[0x3e30_0000, 0x4086_2e42)`. That excludes NaN, infinities, the overflow and underflow thresholds
/// and the tiny-|x| branch, which the scalar takes.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn exp_in_domain(x: __m256d) -> bool {
    unsafe {
        let hx = _mm256_and_si256(_mm256_srli_epi64::<32>(bits(x)), ei(0x7fff_ffff));
        let ok = _mm256_and_si256(_mm256_cmpgt_epi64(hx, ei(0x3e2f_ffff)), _mm256_cmpgt_epi64(ei(0x4086_2e42), hx));
        _mm256_movemask_pd(fbits(ok)) == 0xf
    }
}

/// The fast path of `exp`, lane by lane. Operations and their order are the scalar's; the scalar's
/// branches on `hx`, on `k` and on the sign become selects.
///
/// # Safety
/// The CPU has AVX2 and FMA, and every lane is in the domain of `exp_in_domain`.
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn exp_fast(x: __m256d) -> __m256d {
    unsafe {
        let b = bits(x);
        let hx = _mm256_and_si256(_mm256_srli_epi64::<32>(b), ei(0x7fff_ffff));
        let xsb = _mm256_srli_epi64::<63>(b); // the sign bit, 0 or 1
        let neg = fbits(_mm256_cmpgt_epi64(ei(0), b)); // sign mask, for the selects

        let big = _mm256_cmpgt_epi64(hx, ei(0x3fd6_2e42)); // scalar: `if hx > 0x3fd62e42`, reduce
        let small = _mm256_cmpgt_epi64(ei(0x3ff0_a2b2), hx); // scalar: `hx < 0x3ff0a2b2`

        // |x| < 1.5: h = x - LN2HI[s], l = LN2LO[s], k = 1 - 2s.
        let h1 = _mm256_sub_pd(x, sel(neg, pd(EXP_LN2HI[1]), pd(EXP_LN2HI[0])));
        let l1 = sel(neg, pd(EXP_LN2LO[1]), pd(EXP_LN2LO[0]));
        let k1 = _mm256_sub_epi64(ei(1), _mm256_slli_epi64::<1>(xsb));

        // otherwise: k = (INVLN2*x + half) as i32 (truncates), t = k, h = x - t*LN2HI[0], l = t*LN2LO[0].
        let half = sel(neg, pd(-0.5), pd(0.5));
        let prod = _mm256_add_pd(_mm256_mul_pd(pd(EXP_INVLN2), x), half);
        let k2 = _mm256_cvtepi32_epi64(_mm256_cvttpd_epi32(prod));
        let t2 = i64_to_f64(k2);
        let h2 = _mm256_sub_pd(x, _mm256_mul_pd(t2, pd(EXP_LN2HI[0])));
        let l2 = _mm256_mul_pd(t2, pd(EXP_LN2LO[0]));

        let h_r = sel(fbits(small), h1, h2);
        let l_r = sel(fbits(small), l1, l2);
        let k_r = seli(small, k1, k2);

        // Outside `big` the scalar keeps x and k = 0.
        let k = seli(big, k_r, ei(0));
        let xr = sel(fbits(big), _mm256_sub_pd(h_r, l_r), x);

        // c = x - t*(P1 + t*(P2 + t*(P3 + t*(P4 + t*P5)))), t = x*x
        let t = _mm256_mul_pd(xr, xr);
        let p45 = _mm256_add_pd(pd(EXP_P4), _mm256_mul_pd(t, pd(EXP_P5)));
        let p345 = _mm256_add_pd(pd(EXP_P3), _mm256_mul_pd(t, p45));
        let p2345 = _mm256_add_pd(pd(EXP_P2), _mm256_mul_pd(t, p345));
        let poly = _mm256_add_pd(pd(EXP_P1), _mm256_mul_pd(t, p2345));
        let c = _mm256_sub_pd(xr, _mm256_mul_pd(t, poly));

        let xc = _mm256_mul_pd(xr, c);
        // k == 0: 1 - ((x*c)/(c-2) - x)
        let v0 = _mm256_sub_pd(pd(1.0), _mm256_sub_pd(_mm256_div_pd(xc, _mm256_sub_pd(c, pd(2.0))), xr));
        // else: y = 1 - ((l - (x*c)/(2-c)) - h), then scaled by 2^k
        let y = _mm256_sub_pd(
            pd(1.0),
            _mm256_sub_pd(_mm256_sub_pd(l_r, _mm256_div_pd(xc, _mm256_sub_pd(pd(2.0), c))), h_r),
        );
        let k_ge = _mm256_cmpgt_epi64(k, ei(-1022)); // k >= -1021
        let scaled_hi = add_to_hi(y, _mm256_slli_epi64::<20>(k));
        let scaled_lo = _mm256_mul_pd(
            add_to_hi(y, _mm256_slli_epi64::<20>(_mm256_add_epi64(k, ei(1000)))),
            pd(EXP_TWOM1000),
        );
        let scaled = sel(fbits(k_ge), scaled_hi, scaled_lo);
        let k_zero = _mm256_cmpeq_epi64(k, ei(0));
        sel(fbits(k_zero), v0, scaled)
    }
}

/// `exp` on four lanes. A vector with a lane outside the fast path's domain goes to the scalar `exp`,
/// lane by lane.
///
/// # Safety
/// The CPU has AVX2 and FMA.
#[target_feature(enable = "avx2,fma")]
#[inline]
pub unsafe fn exp_f64x4(x: __m256d) -> __m256d {
    unsafe {
        if exp_in_domain(x) { exp_fast(x) } else { scalar4(x, s::exp) }
    }
}

// ---------------------------------------------------------------------------------------------
// ln (src/math/f64/ln.rs, fdlibm e_log.c)

const LN_LN2_HI: f64 = 6.93147180369123816490e-01;
const LN_LN2_LO: f64 = 1.90821492927058770002e-10;
const LN_LG1: f64 = 6.666666666666735130e-01;
const LN_LG2: f64 = 3.999999999940941908e-01;
const LN_LG3: f64 = 2.857142874366239149e-01;
const LN_LG4: f64 = 2.222219843214978396e-01;
const LN_LG5: f64 = 1.818357216161805012e-01;
const LN_LG6: f64 = 1.531383769920937332e-01;
const LN_LG7: f64 = 1.479819860511658591e-01;

/// The fast path of `ln`: `x` a positive normal number, bits in `[2^52, 0x7ff0_0000_0000_0000)`. Zero,
/// negatives, subnormals, infinities and NaN take the scalar, which returns early or scales `x`.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn ln_in_domain(x: __m256d) -> bool {
    unsafe {
        let b = bits(x);
        let ok = _mm256_and_si256(
            _mm256_cmpgt_epi64(b, ei(0x000f_ffff_ffff_ffff)),
            _mm256_cmpgt_epi64(ei(0x7ff0_0000_0000_0000), b),
        );
        _mm256_movemask_pd(fbits(ok)) == 0xf
    }
}

/// The fast path of `ln`, lane by lane; the scalar's operations in its order, its branches as selects.
///
/// # Safety
/// The CPU has AVX2 and FMA, and every lane is in the domain of `ln_in_domain`.
#[target_feature(enable = "avx2,fma")]
#[inline]
unsafe fn ln_fast(x0: __m256d) -> __m256d {
    unsafe {
        let b = bits(x0);
        let m32 = ei(0xffff_ffff);
        let lx = _mm256_and_si256(b, m32);
        let mut hx = _mm256_srli_epi64::<32>(b); // positive, so the scalar's i32 `hx`
        let mut k = _mm256_sub_epi64(_mm256_srli_epi64::<20>(hx), ei(1023));
        hx = _mm256_and_si256(hx, ei(0x000f_ffff));
        let i0 = _mm256_and_si256(_mm256_add_epi64(hx, ei(0x95f64)), ei(0x0010_0000));
        // x = with_hi(x, hx | (i ^ 0x3ff00000))
        let xhi = _mm256_or_si256(hx, _mm256_xor_si256(i0, ei(0x3ff0_0000)));
        let x = fbits(_mm256_or_si256(_mm256_slli_epi64::<32>(xhi), lx));
        k = _mm256_add_epi64(k, _mm256_srli_epi64::<20>(i0));
        let f = _mm256_sub_pd(x, pd(1.0));
        let kz = _mm256_cmpeq_epi64(k, ei(0));
        let dk = i64_to_f64(k);
        let dk_hi = _mm256_mul_pd(dk, pd(LN_LN2_HI));
        let dk_lo = _mm256_mul_pd(dk, pd(LN_LN2_LO));

        // Small branch: taken when (0xfffff & (2 + hx)) < 3.
        let small = _mm256_cmpgt_epi64(ei(3), _mm256_and_si256(_mm256_add_epi64(ei(2), hx), ei(0x000f_ffff)));
        let fz = _mm256_cmp_pd::<_CMP_EQ_OQ>(f, pd(0.0));
        let s_fz = sel(fbits(kz), pd(0.0), _mm256_add_pd(dk_hi, dk_lo));
        let r = _mm256_mul_pd(
            _mm256_mul_pd(f, f),
            _mm256_sub_pd(pd(0.5), _mm256_mul_pd(pd(0.333_333_333_333_333_33), f)),
        );
        let s_kz = _mm256_sub_pd(f, r);
        let s_kn = _mm256_sub_pd(dk_hi, _mm256_sub_pd(_mm256_sub_pd(r, dk_lo), f));
        let s_nz = sel(fbits(kz), s_kz, s_kn);
        let small_res = sel(fz, s_fz, s_nz);

        // Big branch.
        let s = _mm256_div_pd(f, _mm256_add_pd(pd(2.0), f));
        let z = _mm256_mul_pd(s, s);
        let w = _mm256_mul_pd(z, z);
        let t1 = _mm256_mul_pd(
            w,
            _mm256_add_pd(
                pd(LN_LG2),
                _mm256_mul_pd(w, _mm256_add_pd(pd(LN_LG4), _mm256_mul_pd(w, pd(LN_LG6)))),
            ),
        );
        let inner = _mm256_add_pd(pd(LN_LG5), _mm256_mul_pd(w, pd(LN_LG7)));
        let inner = _mm256_add_pd(pd(LN_LG3), _mm256_mul_pd(w, inner));
        let t2 = _mm256_mul_pd(z, _mm256_add_pd(pd(LN_LG1), _mm256_mul_pd(w, inner)));
        let r2 = _mm256_add_pd(t2, t1);
        // i = (hx - 0x6147a) | (0x6b851 - hx), as i32 in the scalar: here i64 with the same bits.
        let iv = _mm256_or_si256(_mm256_sub_epi64(hx, ei(0x6147a)), _mm256_sub_epi64(ei(0x6b851), hx));
        let pos = _mm256_cmpgt_epi64(iv, ei(0)); // scalar: `if i > 0`
        let hfsq = _mm256_mul_pd(_mm256_mul_pd(pd(0.5), f), f);
        let sr = _mm256_mul_pd(s, _mm256_add_pd(hfsq, r2));
        let p_kz = _mm256_sub_pd(f, _mm256_sub_pd(hfsq, sr));
        let p_kn = _mm256_sub_pd(
            dk_hi,
            _mm256_sub_pd(_mm256_sub_pd(hfsq, _mm256_add_pd(sr, dk_lo)), f),
        );
        let sfr = _mm256_mul_pd(s, _mm256_sub_pd(f, r2));
        let n_kz = _mm256_sub_pd(f, sfr);
        let n_kn = _mm256_sub_pd(dk_hi, _mm256_sub_pd(_mm256_sub_pd(sfr, dk_lo), f));
        let big_res = sel(fbits(pos), sel(fbits(kz), p_kz, p_kn), sel(fbits(kz), n_kz, n_kn));

        sel(fbits(small), small_res, big_res)
    }
}

/// `ln` on four lanes. A vector with a lane outside the fast path's domain goes to the scalar `ln`.
///
/// # Safety
/// The CPU has AVX2 and FMA.
#[target_feature(enable = "avx2,fma")]
#[inline]
pub unsafe fn ln_f64x4(x: __m256d) -> __m256d {
    unsafe {
        if ln_in_domain(x) { ln_fast(x) } else { scalar4(x, s::ln) }
    }
}

// ---------------------------------------------------------------------------------------------
// sin and cos on |x| <= pi/4 (src/math/f64/trig.rs: `sin`, `cos` and `sin_cos` call the kernels
// directly when `ix <= 0x3fe921fb`, with y = 0). Larger arguments need the reduction
// `ieee754_rem_pio2`, which stays in the scalar: a vector with such a lane goes to the scalar.

const SIN_S1: f64 = -0.16666666666666632;
const SIN_S2: f64 = 0.00833333333332249;
const SIN_S3: f64 = -0.0001984126982985795;
const SIN_S4: f64 = 2.7557313707070068e-06;
const SIN_S5: f64 = -2.5050760253406863e-08;
const SIN_S6: f64 = 1.58969099521155e-10;
const COS_C1: f64 = 0.0416666666666666;
const COS_C2: f64 = -0.001388888888887411;
const COS_C3: f64 = 2.480158728947673e-05;
const COS_C4: f64 = -2.7557314351390663e-07;
const COS_C5: f64 = 2.087572321298175e-09;
const COS_C6: f64 = -1.1359647557788195e-11;

/// The scalar's small-argument test, `hi(x) & 0x7fff_ffff <= 0x3fe9_21fb` (|x| <= pi/4), in each lane.
/// NaN and infinities fail it. The lane high word is `hx` (already masked), as in `exp_in_domain`.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn trig_hx(x: __m256d) -> __m256i {
    unsafe { _mm256_and_si256(_mm256_srli_epi64::<32>(bits(x)), ei(0x7fff_ffff)) }
}

/// Whether every lane is in the small-argument domain of `sin` and `cos`.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn trig_small_in_domain(x: __m256d) -> bool {
    unsafe { _mm256_movemask_pd(fbits(_mm256_cmpgt_epi64(ei(0x3fe9_21fc), trig_hx(x)))) == 0xf }
}

/// `kernel_sin(x, 0, 0)` (k_sin.c) per lane: the scalar's `ix < 0x3e40_0000 && x as i32 == 0` return
/// is a select (for `ix < 0x3e40_0000` the `as i32` test always holds, so the select is on `ix` alone).
///
/// # Safety
/// The CPU has AVX2, and every lane has `|x| <= pi/4`.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn sin_small(x: __m256d) -> __m256d {
    unsafe {
        let hx = trig_hx(x);
        let tiny = fbits(_mm256_cmpgt_epi64(ei(0x3e40_0000), hx));
        let z = _mm256_mul_pd(x, x);
        let v = _mm256_mul_pd(z, x);
        // r = S2 + z*(S3 + z*(S4 + z*(S5 + z*S6)))
        let r = _mm256_add_pd(
            pd(SIN_S2),
            _mm256_mul_pd(
                z,
                _mm256_add_pd(
                    pd(SIN_S3),
                    _mm256_mul_pd(
                        z,
                        _mm256_add_pd(
                            pd(SIN_S4),
                            _mm256_mul_pd(z, _mm256_add_pd(pd(SIN_S5), _mm256_mul_pd(z, pd(SIN_S6)))),
                        ),
                    ),
                ),
            ),
        );
        // iy == 0: x + v*(S1 + z*r)
        let res = _mm256_add_pd(x, _mm256_mul_pd(v, _mm256_add_pd(pd(SIN_S1), _mm256_mul_pd(z, r))));
        sel(tiny, x, res)
    }
}

/// `kernel_cos(x, 0)` (k_cos.c) per lane. The scalar's `x*y` is `x * 0.0` here, kept so that the sign of
/// a zero result is the scalar's. Its three branches on `ix` are selects.
///
/// # Safety
/// The CPU has AVX2, and every lane has `|x| <= pi/4`.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn cos_small(x: __m256d) -> __m256d {
    unsafe {
        let hx = trig_hx(x);
        let tiny = fbits(_mm256_cmpgt_epi64(ei(0x3e40_0000), hx));
        let z = _mm256_mul_pd(x, x);
        // r = z*(C1 + z*(C2 + z*(C3 + z*(C4 + z*(C5 + z*C6)))))
        let p = _mm256_add_pd(pd(COS_C5), _mm256_mul_pd(z, pd(COS_C6)));
        let p = _mm256_add_pd(pd(COS_C4), _mm256_mul_pd(z, p));
        let p = _mm256_add_pd(pd(COS_C3), _mm256_mul_pd(z, p));
        let p = _mm256_add_pd(pd(COS_C2), _mm256_mul_pd(z, p));
        let p = _mm256_add_pd(pd(COS_C1), _mm256_mul_pd(z, p));
        let r = _mm256_mul_pd(z, p);
        let xy = _mm256_mul_pd(x, pd(0.0)); // y = ZERO
        let zr_xy = _mm256_sub_pd(_mm256_mul_pd(z, r), xy);
        // ix < 0x3FD33333: ONE - (HALF*z - (z*r - x*y))
        let small = fbits(_mm256_cmpgt_epi64(ei(0x3fd3_3333), hx));
        let v_small = _mm256_sub_pd(pd(1.0), _mm256_sub_pd(_mm256_mul_pd(pd(0.5), z), zr_xy));
        // else: qx = 0.28125 if ix > 0x3fe90000, else the double with high word ix - 0x200000;
        // a = ONE - qx; a - (hz - (z*r - x*y)) with hz = HALF*z - qx.
        let qx_big = pd(0.28125);
        let qx_mid = fbits(_mm256_slli_epi64::<32>(_mm256_sub_epi64(hx, ei(0x0020_0000))));
        let qx = sel(fbits(_mm256_cmpgt_epi64(hx, ei(0x3fe9_0000))), qx_big, qx_mid);
        let hz = _mm256_sub_pd(_mm256_mul_pd(pd(0.5), z), qx);
        let a = _mm256_sub_pd(pd(1.0), qx);
        let v_mid = _mm256_sub_pd(a, _mm256_sub_pd(hz, zr_xy));
        let res = sel(small, v_small, v_mid);
        sel(tiny, pd(1.0), res)
    }
}

/// `sin` on four lanes for |x| <= pi/4; a vector with a lane outside goes to the scalar `sin`.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn sin_f64x4(x: __m256d) -> __m256d {
    unsafe { if trig_small_in_domain(x) { sin_small(x) } else { scalar4(x, s::sin) } }
}

/// `cos` on four lanes for |x| <= pi/4; a vector with a lane outside goes to the scalar `cos`.
///
/// # Safety
/// The CPU has AVX2.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn cos_f64x4(x: __m256d) -> __m256d {
    unsafe { if trig_small_in_domain(x) { cos_small(x) } else { scalar4(x, s::cos) } }
}

// ---------------------------------------------------------------------------------------------
// Slices. Same length asserted; the tail (fewer than four elements) is the scalar function.

macro_rules! slice_fn {
    ($(#[$m:meta])* $name:ident, $imp:ident, $vec:ident, $sc:path) => {
        $(#[$m])*
        pub fn $name(xs: &[f64], out: &mut [f64]) {
            assert_eq!(xs.len(), out.len(), "trame: lanes: input and output lengths differ");
            if has_avx2_fma() {
                // SAFETY: has_avx2_fma() is true, so AVX2 and FMA are present.
                unsafe { $imp(xs, out) }
            } else {
                for (o, x) in out.iter_mut().zip(xs) {
                    *o = $sc(*x);
                }
            }
        }

        #[target_feature(enable = "avx2,fma")]
        unsafe fn $imp(xs: &[f64], out: &mut [f64]) {
            unsafe {
                let n = xs.len();
                let mut i = 0;
                while i + 4 <= n {
                    let v = _mm256_loadu_pd(xs.as_ptr().add(i));
                    _mm256_storeu_pd(out.as_mut_ptr().add(i), $vec(v));
                    i += 4;
                }
                while i < n {
                    out[i] = $sc(xs[i]);
                    i += 1;
                }
            }
        }
    };
}

slice_fn!(
    /// `out[i] = exp(xs[i])`, bit-identical to `trame::math::f64::exp`.
    exp_slice, exp_slice_imp, exp_f64x4, s::exp
);
slice_fn!(
    /// `out[i] = ln(xs[i])`, bit-identical to `trame::math::f64::ln`.
    ln_slice, ln_slice_imp, ln_f64x4, s::ln
);
slice_fn!(
    /// `out[i] = sin(xs[i])`, bit-identical to `trame::math::f64::sin`.
    sin_slice, sin_slice_imp, sin_f64x4, s::sin
);
slice_fn!(
    /// `out[i] = cos(xs[i])`, bit-identical to `trame::math::f64::cos`.
    cos_slice, cos_slice_imp, cos_f64x4, s::cos
);
slice_fn!(
    /// `out[i] = sqrt(xs[i])`, bit-identical to `trame::math::f64::sqrt`.
    sqrt_slice, sqrt_slice_imp, sqrt_f64x4, s::sqrt
);
slice_fn!(
    /// `out[i] = floor(xs[i])`, bit-identical to `trame::math::f64::floor`.
    floor_slice, floor_slice_imp, floor_f64x4, s::floor
);
slice_fn!(
    /// `out[i] = ceil(xs[i])`, bit-identical to `trame::math::f64::ceil`.
    ceil_slice, ceil_slice_imp, ceil_f64x4, s::ceil
);
slice_fn!(
    /// `out[i] = trunc(xs[i])`, bit-identical to `trame::math::f64::trunc`.
    trunc_slice, trunc_slice_imp, trunc_f64x4, s::trunc
);
slice_fn!(
    /// `out[i] = round_ties_even(xs[i])`, bit-identical to `trame::math::f64::round_ties_even`.
    round_ties_even_slice, round_ties_even_slice_imp, round_ties_even_f64x4, s::round_ties_even
);

/// `out[i] = max(a[i], b[i])`, bit-identical to `trame::math::f64::max`.
pub fn max_slice(a: &[f64], b: &[f64], out: &mut [f64]) {
    assert!(a.len() == b.len() && a.len() == out.len(), "trame: lanes: lengths differ");
    if has_avx2_fma() {
        // SAFETY: has_avx2_fma() is true.
        unsafe { max_slice_imp(a, b, out) }
    } else {
        for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
            *o = s::max(*x, *y);
        }
    }
}

#[target_feature(enable = "avx2,fma")]
unsafe fn max_slice_imp(a: &[f64], b: &[f64], out: &mut [f64]) {
    unsafe {
        let n = a.len();
        let mut i = 0;
        while i + 4 <= n {
            let v = max_f64x4(_mm256_loadu_pd(a.as_ptr().add(i)), _mm256_loadu_pd(b.as_ptr().add(i)));
            _mm256_storeu_pd(out.as_mut_ptr().add(i), v);
            i += 4;
        }
        while i < n {
            out[i] = s::max(a[i], b[i]);
            i += 1;
        }
    }
}

/// `out[i] = min(a[i], b[i])`, bit-identical to `trame::math::f64::min`.
pub fn min_slice(a: &[f64], b: &[f64], out: &mut [f64]) {
    assert!(a.len() == b.len() && a.len() == out.len(), "trame: lanes: lengths differ");
    if has_avx2_fma() {
        // SAFETY: has_avx2_fma() is true.
        unsafe { min_slice_imp(a, b, out) }
    } else {
        for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
            *o = s::min(*x, *y);
        }
    }
}

#[target_feature(enable = "avx2,fma")]
unsafe fn min_slice_imp(a: &[f64], b: &[f64], out: &mut [f64]) {
    unsafe {
        let n = a.len();
        let mut i = 0;
        while i + 4 <= n {
            let v = min_f64x4(_mm256_loadu_pd(a.as_ptr().add(i)), _mm256_loadu_pd(b.as_ptr().add(i)));
            _mm256_storeu_pd(out.as_mut_ptr().add(i), v);
            i += 4;
        }
        while i < n {
            out[i] = s::min(a[i], b[i]);
            i += 1;
        }
    }
}

/// `out[i] = a[i]*b[i] + c[i]` with one rounding, bit-identical to `trame::math::f64::mul_add`.
pub fn mul_add_slice(a: &[f64], b: &[f64], c: &[f64], out: &mut [f64]) {
    assert!(a.len() == b.len() && a.len() == c.len() && a.len() == out.len(), "trame: lanes: lengths differ");
    if has_avx2_fma() {
        // SAFETY: has_avx2_fma() is true.
        unsafe { mul_add_slice_imp(a, b, c, out) }
    } else {
        for (((o, x), y), z) in out.iter_mut().zip(a).zip(b).zip(c) {
            *o = s::mul_add(*x, *y, *z);
        }
    }
}

#[target_feature(enable = "avx2,fma")]
unsafe fn mul_add_slice_imp(a: &[f64], b: &[f64], c: &[f64], out: &mut [f64]) {
    unsafe {
        let n = a.len();
        let mut i = 0;
        while i + 4 <= n {
            let v = mul_add_f64x4(
                _mm256_loadu_pd(a.as_ptr().add(i)),
                _mm256_loadu_pd(b.as_ptr().add(i)),
                _mm256_loadu_pd(c.as_ptr().add(i)),
            );
            _mm256_storeu_pd(out.as_mut_ptr().add(i), v);
            i += 4;
        }
        while i < n {
            out[i] = s::mul_add(a[i], b[i], c[i]);
            i += 1;
        }
    }
}
