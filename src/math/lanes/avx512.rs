#![allow(unsafe_op_in_unsafe_fn)] // each unsafe fn body is one target-feature region; its preconditions are on the fn

//! AVX-512 lane versions of the exact `trame::math::f64` functions: sqrt, floor, ceil, trunc,
//! round_ties_even, max, min, mul_add. Eight f64 lanes per vector. Written for x86-64-v4 (F, BW, DQ, VL),
//! the feature set `lanes-avx512` names. NOT RUN: the development CPU (AMD Ryzen 5 PRO 4650U, Zen 2) has no
//! AVX-512, so this file is compiled (`--features seq,lanes-avx512`) and never executed. The exp/ln/trig
//! lanes are not written for AVX-512.
//!
//! The exactness argument is the one in `avx2.rs`: each function is the scalar's operations per lane,
//! and a select (`mask_blend`) is a scalar `if`. No `fmadd` except in `mul_add`.
//!
//! Preconditions. Every `unsafe fn` needs AVX-512 F, BW, DQ and VL, and the OS saving the ZMM state.
//! The safe slice functions check it with `has_avx512()`.

use crate::math::f64 as s;
use core::arch::x86_64::*;
use core::sync::atomic::{AtomicU8, Ordering};

/// Whether the CPU and OS run AVX-512 F, BW, DQ and VL. Probed once and cached.
pub fn has_avx512() -> bool {
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
        let f = l7.ebx & (1 << 16) != 0;
        let dq = l7.ebx & (1 << 17) != 0;
        let bw = l7.ebx & (1 << 30) != 0;
        let vl = l7.ebx & (1 << 31) != 0;
        if !(osxsave && f && dq && bw && vl) {
            return false;
        }
        // SAFETY: OSXSAVE is set, so xgetbv is legal.
        (xgetbv0() & 0xe6) == 0xe6 // XMM, YMM, opmask, ZMM_Hi256, Hi16_ZMM state
    }
}

#[target_feature(enable = "xsave")]
unsafe fn xgetbv0() -> u64 {
    // SAFETY: the caller checked OSXSAVE.
    unsafe { _xgetbv(0) }
}

/// `m ? t : f` per lane (a mask bit set takes `t`).
#[target_feature(enable = "avx512f")]
#[inline]
unsafe fn sel(m: __mmask8, t: __m512d, f: __m512d) -> __m512d {
    _mm512_mask_blend_pd(m, f, t)
}

#[target_feature(enable = "avx512f")]
#[inline]
unsafe fn canon(r: __m512d) -> __m512d {
    let is_nan = _mm512_cmp_pd_mask::<_CMP_UNORD_Q>(r, r);
    sel(is_nan, _mm512_set1_pd(f64::NAN), r)
}

/// `sqrt` (sqrt.rs): `vsqrtpd` is correctly rounded; negatives get the scalar's `f64::NAN`.
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn sqrt_f64x8(x: __m512d) -> __m512d {
    let r = _mm512_sqrt_pd(x);
    let neg = _mm512_cmp_pd_mask::<_CMP_LT_OQ>(x, _mm512_setzero_pd());
    sel(neg, _mm512_set1_pd(f64::NAN), r)
}

/// `floor` (floor.rs). `roundscale` with scale 0 is the round-to-integer of the immediate's mode.
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn floor_f64x8(x: __m512d) -> __m512d {
    _mm512_roundscale_pd::<{ _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }>(x)
}

/// `ceil` (floor.rs).
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn ceil_f64x8(x: __m512d) -> __m512d {
    _mm512_roundscale_pd::<{ _MM_FROUND_TO_POS_INF | _MM_FROUND_NO_EXC }>(x)
}

/// `trunc` (floor.rs).
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn trunc_f64x8(x: __m512d) -> __m512d {
    _mm512_roundscale_pd::<{ _MM_FROUND_TO_ZERO | _MM_FROUND_NO_EXC }>(x)
}

/// `round_ties_even` (floor.rs).
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn round_ties_even_f64x8(x: __m512d) -> __m512d {
    _mm512_roundscale_pd::<{ _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC }>(x)
}

/// `max` (minmax.rs), lane by lane; the scalar's NaN rule and zero-sign rule as selects.
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn max_f64x8(a: __m512d, b: __m512d) -> __m512d {
    let a_nan = _mm512_cmp_pd_mask::<_CMP_UNORD_Q>(a, a);
    let b_nan = _mm512_cmp_pd_mask::<_CMP_UNORD_Q>(b, b);
    let gt = _mm512_cmp_pd_mask::<_CMP_GT_OQ>(a, b);
    let lt = _mm512_cmp_pd_mask::<_CMP_GT_OQ>(b, a);
    let b_neg = _mm512_movepi64_mask(_mm512_castpd_si512(b));
    let eq = sel(b_neg, a, b);
    let main = sel(gt, a, sel(lt, b, eq));
    let t = sel(b_nan, a, main);
    sel(a_nan, b, t)
}

/// `min` (minmax.rs), lane by lane.
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn min_f64x8(a: __m512d, b: __m512d) -> __m512d {
    let a_nan = _mm512_cmp_pd_mask::<_CMP_UNORD_Q>(a, a);
    let b_nan = _mm512_cmp_pd_mask::<_CMP_UNORD_Q>(b, b);
    let lt = _mm512_cmp_pd_mask::<_CMP_LT_OQ>(a, b);
    let gt = _mm512_cmp_pd_mask::<_CMP_LT_OQ>(b, a);
    let a_neg = _mm512_movepi64_mask(_mm512_castpd_si512(a));
    let eq = sel(a_neg, a, b);
    let main = sel(lt, a, sel(gt, b, eq));
    let t = sel(b_nan, a, main);
    sel(a_nan, b, t)
}

/// `mul_add` (fma.rs): `vfmadd` rounds once; NaN results are `f64::NAN`.
///
/// # Safety
/// The CPU has AVX-512 F, BW, DQ, VL.
#[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
#[inline]
pub unsafe fn mul_add_f64x8(a: __m512d, b: __m512d, c: __m512d) -> __m512d {
    canon(_mm512_fmadd_pd(a, b, c))
}

macro_rules! slice8 {
    ($name:ident, $imp:ident, $sc:path, $vec:ident, $($arg:ident),+) => {
        /// Lane version over slices; same lengths asserted; the tail is the scalar function.
        pub fn $name($($arg: &[f64]),+, out: &mut [f64]) {
            $(assert_eq!($arg.len(), out.len(), "trame: lanes: lengths differ");)+
            if has_avx512() {
                // SAFETY: has_avx512() is true.
                unsafe { $imp($($arg),+, out) }
            } else {
                let n = out.len();
                for i in 0..n {
                    out[i] = $sc($($arg[i]),+);
                }
            }
        }

        #[target_feature(enable = "avx512f,avx512dq,avx512bw,avx512vl")]
        unsafe fn $imp($($arg: &[f64]),+, out: &mut [f64]) {
            let n = out.len();
            let mut i = 0;
            while i + 8 <= n {
                // SAFETY: i + 8 <= n, so the 8 elements at i are in each slice.
                unsafe {
                    let v = $vec($(_mm512_loadu_pd($arg.as_ptr().add(i))),+);
                    _mm512_storeu_pd(out.as_mut_ptr().add(i), v);
                }
                i += 8;
            }
            while i < n {
                out[i] = $sc($($arg[i]),+);
                i += 1;
            }
        }
    };
}

slice8!(sqrt_slice, sqrt_slice_imp, s::sqrt, sqrt_f64x8, x);
slice8!(floor_slice, floor_slice_imp, s::floor, floor_f64x8, x);
slice8!(ceil_slice, ceil_slice_imp, s::ceil, ceil_f64x8, x);
slice8!(trunc_slice, trunc_slice_imp, s::trunc, trunc_f64x8, x);
slice8!(round_ties_even_slice, round_ties_even_slice_imp, s::round_ties_even, round_ties_even_f64x8, x);
slice8!(max_slice, max_slice_imp, s::max, max_f64x8, a, b);
slice8!(min_slice, min_slice_imp, s::min, min_f64x8, a, b);
slice8!(mul_add_slice, mul_add_slice_imp, s::mul_add, mul_add_f64x8, a, b, c);
