//! AVX2 lane versions. Every `unsafe fn` here has the preconditions stated on it; the safe entry
//! points at the end establish them (CPU has AVX2, `0 < n <= i32::MAX`) and the kernels establish
//! the rest with the key check of pass 1 (every key below `n`, before any slot is written).
//!
//! Exactness rules used by the lane bodies:
//! * one lane operation per scalar operation, the same operands in the same order, IEEE round to
//!   nearest: `_mm256_mul_ps` and `_mm256_add_ps` are the lanes of `mulss` and `addss`;
//! * no fma unless the scalar body calls `mul_add`; no reassociation; no reciprocal;
//! * a branch is a select, and a select discards the lane, it does not compute with it;
//! * integer add that aborts on overflow is an add plus a sticky fault mask; a fault sends the
//!   window back through the scalar item, which aborts with the scalar message.

use crate::items::*;
use crate::plan::Class;
use core::arch::x86_64::*;

pub fn has_avx2() -> bool {
    is_x86_feature_detected!("avx2")
}

// ---------------------------------------------------------------------------------------------
// f32, 8 lanes, items {index: u32, x: f32}

/// Load 8 consecutive `Xi` and split them into keys and values, in order.
/// Safety: `p..p+8` is readable.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn load8(p: *const Xi) -> (__m256i, __m256) {
    unsafe {
        let a = _mm256_loadu_ps(p as *const f32); // k0 x0 k1 x1 | k2 x2 k3 x3
        let b = _mm256_loadu_ps(p.add(4) as *const f32); // k4 x4 k5 x5 | k6 x6 k7 x7
        let k = _mm256_shuffle_ps::<0x88>(a, b); // k0 k1 k4 k5 | k2 k3 k6 k7
        let x = _mm256_shuffle_ps::<0xDD>(a, b);
        let k = _mm256_permute4x64_epi64::<0xD8>(_mm256_castps_si256(k));
        let x = _mm256_castsi256_ps(_mm256_permute4x64_epi64::<0xD8>(_mm256_castps_si256(x)));
        (k, x)
    }
}

/// Pairwise distinct: compare each lane with the lanes 1, 2, 3, 4 places on (mod 8); every
/// unordered pair of lanes is at distance 1..=4 one way or the other.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn distinct8(k: __m256i) -> bool {
    unsafe {
        let r1 = _mm256_set_epi32(0, 7, 6, 5, 4, 3, 2, 1);
        let r2 = _mm256_set_epi32(1, 0, 7, 6, 5, 4, 3, 2);
        let r3 = _mm256_set_epi32(2, 1, 0, 7, 6, 5, 4, 3);
        let r4 = _mm256_set_epi32(3, 2, 1, 0, 7, 6, 5, 4);
        let e = _mm256_or_si256(
            _mm256_or_si256(
                _mm256_cmpeq_epi32(k, _mm256_permutevar8x32_epi32(k, r1)),
                _mm256_cmpeq_epi32(k, _mm256_permutevar8x32_epi32(k, r2)),
            ),
            _mm256_or_si256(
                _mm256_cmpeq_epi32(k, _mm256_permutevar8x32_epi32(k, r3)),
                _mm256_cmpeq_epi32(k, _mm256_permutevar8x32_epi32(k, r4)),
            ),
        );
        _mm256_testz_si256(e, e) == 1
    }
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn contiguous8(k: __m256i) -> bool {
    unsafe {
        let iota = _mm256_set_epi32(7, 6, 5, 4, 3, 2, 1, 0);
        let k0 = _mm256_set1_epi32(_mm256_cvtsi256_si32(k));
        let e = _mm256_cmpeq_epi32(k, _mm256_add_epi32(k0, iota));
        _mm256_movemask_ps(_mm256_castsi256_ps(e)) == 0xFF
    }
}

/// The class of a window of 8 keys, the vector code of the kernels. Checked against
/// `plan::classify` in tests/exhaustive.rs.
#[target_feature(enable = "avx2")]
pub unsafe fn classify8(keys: &[u32; 8]) -> Class {
    unsafe {
        let k = _mm256_loadu_si256(keys.as_ptr() as *const __m256i);
        if contiguous8(k) {
            Class::Contiguous
        } else if distinct8(k) {
            Class::Distinct
        } else {
            Class::Conflict
        }
    }
}

/// Pass 1: is every key below `n`? Unsigned max over the lanes. Precondition: `0 < n <= i32::MAX`.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn keys_ok8(items: &[Xi], n: usize) -> bool {
    unsafe {
        let nm1 = _mm256_set1_epi32((n - 1) as i32);
        let mut mx = _mm256_setzero_si256();
        let mut i = 0;
        while i + 8 <= items.len() {
            let (k, _) = load8(items.as_ptr().add(i));
            mx = _mm256_max_epu32(mx, k);
            i += 8;
        }
        let ok = _mm256_cmpeq_epi32(_mm256_max_epu32(mx, nm1), nm1);
        let mut all = _mm256_movemask_ps(_mm256_castsi256_ps(ok)) == 0xFF;
        for it in &items[i..] {
            all &= (it.index as usize) < n;
        }
        all
    }
}

macro_rules! window_f32 {
    ($(#[$m:meta])* $name:ident, $cx:ty, $item:ident, $lane:ident) => {
        $(#[$m])*
        ///
        /// Safety: AVX2 is present; `0 < slots.len() <= i32::MAX`.
        #[target_feature(enable = "avx2")]
        pub unsafe fn $name(cx: &$cx, items: &[Xi], slots: &mut [f32]) {
            unsafe {
                let n = slots.len();
                if !keys_ok8(items, n) {
                    // Pass 1 failed: abort with the scalar message, no item has run.
                    check_keys_scalar(|i: &Xi| i.index as usize, items, n);
                    unreachable!();
                }
                // Every key is below n from here on. That is what makes the raw accesses below
                // in bounds: a contiguous window's last slot is k0 + 7 < n because the keys are
                // k0..k0+7 and all below n; gathered and stored slots are keys, below n.
                let sp = slots.as_mut_ptr();
                let ip = items.as_ptr();
                let mut i = 0;
                while i + 8 <= items.len() {
                    let (k, x) = load8(ip.add(i));
                    if contiguous8(k) {
                        let k0 = _mm256_cvtsi256_si32(k) as u32 as usize;
                        let s = _mm256_loadu_ps(sp.add(k0));
                        _mm256_storeu_ps(sp.add(k0), $lane(cx, x, s));
                    } else if distinct8(k) {
                        let s = _mm256_i32gather_ps::<4>(sp as *const f32, k);
                        let r = $lane(cx, x, s);
                        let mut ks = [0u32; 8];
                        let mut rs = [0f32; 8];
                        _mm256_storeu_si256(ks.as_mut_ptr() as *mut __m256i, k);
                        _mm256_storeu_ps(rs.as_mut_ptr(), r);
                        for j in 0..8 {
                            *sp.add(ks[j] as usize) = rs[j];
                        }
                    } else {
                        for j in 0..8 {
                            let it = &*ip.add(i + j);
                            $item(it, &mut *sp.add(it.index as usize), cx);
                        }
                    }
                    i += 8;
                }
                while i < items.len() {
                    let it = &*ip.add(i);
                    $item(it, &mut *sp.add(it.index as usize), cx);
                    i += 1;
                }
            }
        }
    };
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn axpy_lane(cx: &Cx, x: __m256, s: __m256) -> __m256 {
    _mm256_add_ps(s, _mm256_mul_ps(_mm256_set1_ps(cx.a), x))
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn axpy_sel_lane(cx: &Cx, x: __m256, s: __m256) -> __m256 {
    unsafe {
        let m = _mm256_cmp_ps::<_CMP_GT_OQ>(x, _mm256_set1_ps(cx.t));
        let r = _mm256_add_ps(s, _mm256_mul_ps(_mm256_set1_ps(cx.a), x));
        _mm256_blendv_ps(s, r, m)
    }
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn poly_lane(cx: &Cp, x: __m256, s: __m256) -> __m256 {
    unsafe {
        let mut p = _mm256_set1_ps(cx.c[7]);
        for i in (0..7).rev() {
            p = _mm256_add_ps(_mm256_mul_ps(p, x), _mm256_set1_ps(cx.c[i]));
        }
        _mm256_add_ps(s, p)
    }
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn sum32_lane(_cx: &(), x: __m256, s: __m256) -> __m256 {
    _mm256_add_ps(s, x)
}

window_f32!(
    /// `parallel!(axpy, ..)`, window kernel.
    axpy_window, Cx, axpy, axpy_lane
);
window_f32!(
    /// `parallel!(axpy_sel, ..)`, window kernel.
    axpy_sel_window, Cx, axpy_sel, axpy_sel_lane
);
window_f32!(
    /// `parallel!(poly, ..)`, window kernel.
    poly_window, Cp, poly, poly_lane
);
window_f32!(
    /// `parallel!(sum32, ..)`, window kernel.
    sum32_window, (), sum32, sum32_lane
);

/// `parallel!(sum32, ..)`, register kernel: `n <= 8 * A` slots live in `A` registers. Each item
/// adds its value to the lane whose id is its key, and `-0.0` to every other lane: `x + (-0.0)` is
/// `x` for every x that is not a signalling NaN, including `+0.0` and `-0.0` (a `+0.0` would turn
/// a `-0.0` slot into `+0.0`). The slots are written back at the end.
///
/// Safety: AVX2 is present; `0 < slots.len() <= 8 * A`, `A <= 8`.
#[target_feature(enable = "avx2")]
pub unsafe fn sum32_reg<const A: usize>(items: &[Xi], slots: &mut [f32]) {
    unsafe {
        let n = slots.len();
        if !keys_ok8(items, n) {
            check_keys_scalar(|i: &Xi| i.index as usize, items, n);
            unreachable!();
        }
        let mut buf = [0f32; 64];
        buf[..n].copy_from_slice(slots);
        let mut acc = [_mm256_setzero_ps(); A];
        let mut ids = [_mm256_setzero_si256(); A];
        for a in 0..A {
            acc[a] = _mm256_loadu_ps(buf.as_ptr().add(8 * a));
            ids[a] = _mm256_add_epi32(
                _mm256_set_epi32(7, 6, 5, 4, 3, 2, 1, 0),
                _mm256_set1_epi32(8 * a as i32),
            );
        }
        let negz = _mm256_set1_ps(-0.0);
        for it in items {
            let kb = _mm256_set1_epi32(it.index as i32);
            let vb = _mm256_set1_ps(it.x);
            for a in 0..A {
                let m = _mm256_castsi256_ps(_mm256_cmpeq_epi32(kb, ids[a]));
                acc[a] = _mm256_add_ps(acc[a], _mm256_blendv_ps(negz, vb, m));
            }
        }
        for a in 0..A {
            _mm256_storeu_ps(buf.as_mut_ptr().add(8 * a), acc[a]);
        }
        slots.copy_from_slice(&buf[..n]);
    }
}

// ---------------------------------------------------------------------------------------------
// u64, 4 lanes, items {index: u32, _p: u32, v: u64}

/// Safety: `p..p+4` is readable.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn load4(p: *const Kv64) -> (__m256i, __m256i) {
    unsafe {
        let a = _mm256_loadu_si256(p as *const __m256i); // i0 | i1
        let b = _mm256_loadu_si256(p.add(2) as *const __m256i); // i2 | i3
        let k = _mm256_permute4x64_epi64::<0xD8>(_mm256_unpacklo_epi64(a, b));
        let v = _mm256_permute4x64_epi64::<0xD8>(_mm256_unpackhi_epi64(a, b));
        (_mm256_and_si256(k, _mm256_set1_epi64x(0xFFFF_FFFF)), v)
    }
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn distinct4(k: __m256i) -> bool {
    unsafe {
        let e = _mm256_or_si256(
            _mm256_cmpeq_epi64(k, _mm256_permute4x64_epi64::<0x39>(k)), // rotate by 1
            _mm256_cmpeq_epi64(k, _mm256_permute4x64_epi64::<0x4E>(k)), // rotate by 2
        );
        _mm256_testz_si256(e, e) == 1
    }
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn contiguous4(k: __m256i) -> bool {
    unsafe {
        let k0 = _mm256_set1_epi64x(_mm256_extract_epi64::<0>(k));
        let e = _mm256_cmpeq_epi64(k, _mm256_add_epi64(k0, _mm256_set_epi64x(3, 2, 1, 0)));
        _mm256_movemask_pd(_mm256_castsi256_pd(e)) == 0xF
    }
}

#[target_feature(enable = "avx2")]
#[inline]
unsafe fn keys_ok4(items: &[Kv64], n: usize) -> bool {
    unsafe {
        let nm1 = _mm256_set1_epi64x((n - 1) as i64);
        let mut bad = _mm256_setzero_si256();
        let mut i = 0;
        while i + 4 <= items.len() {
            let (k, _) = load4(items.as_ptr().add(i));
            bad = _mm256_or_si256(bad, _mm256_cmpgt_epi64(k, nm1)); // keys < 2^32: signed compare is exact
            i += 4;
        }
        let mut all = _mm256_testz_si256(bad, bad) == 1;
        for it in &items[i..] {
            all &= (it.index as usize) < n;
        }
        all
    }
}

const BIAS: i64 = i64::MIN;

/// `parallel!(sum64, ..)`, window kernel. A window with an overflowing lane goes through the scalar
/// item, which aborts with the scalar message; its slots are untouched until then, so the state at
/// the abort is the scalar fold's.
///
/// Safety: AVX2 is present; `0 < slots.len() <= i32::MAX`.
#[target_feature(enable = "avx2")]
pub unsafe fn sum64_window(items: &[Kv64], slots: &mut [u64]) {
    unsafe {
        let n = slots.len();
        if !keys_ok4(items, n) {
            check_keys_scalar(|i: &Kv64| i.index as usize, items, n);
            unreachable!();
        }
        let sp = slots.as_mut_ptr();
        let ip = items.as_ptr();
        let bias = _mm256_set1_epi64x(BIAS);
        let mut i = 0;
        while i + 4 <= items.len() {
            let (k, v) = load4(ip.add(i));
            let cont = contiguous4(k);
            if cont || distinct4(k) {
                let s = if cont {
                    _mm256_loadu_si256(sp.add(_mm256_extract_epi64::<0>(k) as usize) as *const __m256i)
                } else {
                    _mm256_i64gather_epi64::<8>(sp as *const i64, k)
                };
                let r = _mm256_add_epi64(s, v);
                // unsigned r < s: the add wrapped
                let ovf = _mm256_cmpgt_epi64(_mm256_xor_si256(s, bias), _mm256_xor_si256(r, bias));
                if _mm256_testz_si256(ovf, ovf) == 1 {
                    if cont {
                        _mm256_storeu_si256(sp.add(_mm256_extract_epi64::<0>(k) as usize) as *mut __m256i, r);
                    } else {
                        let mut ks = [0u64; 4];
                        let mut rs = [0u64; 4];
                        _mm256_storeu_si256(ks.as_mut_ptr() as *mut __m256i, k);
                        _mm256_storeu_si256(rs.as_mut_ptr() as *mut __m256i, r);
                        for j in 0..4 {
                            *sp.add(ks[j] as usize) = rs[j];
                        }
                    }
                    i += 4;
                    continue;
                }
            }
            for j in 0..4 {
                let it = &*ip.add(i + j);
                sum64(it, &mut *sp.add(it.index as usize), &());
            }
            i += 4;
        }
        while i < items.len() {
            let it = &*ip.add(i);
            sum64(it, &mut *sp.add(it.index as usize), &());
            i += 1;
        }
    }
}

/// `parallel!(sum64, ..)`, register kernel: `n <= 4 * A` slots in `A` registers, biased by 2^63 so
/// that "the add wrapped" is one signed compare. A wrap anywhere discards the registers and reruns
/// the scalar fold from the untouched slots.
///
/// Safety: AVX2 is present; `0 < slots.len() <= 4 * A`, `A <= 16`.
#[target_feature(enable = "avx2")]
pub unsafe fn sum64_reg<const A: usize>(items: &[Kv64], slots: &mut [u64]) {
    unsafe {
        let n = slots.len();
        if !keys_ok4(items, n) {
            check_keys_scalar(|i: &Kv64| i.index as usize, items, n);
            unreachable!();
        }
        let mut buf = [0u64; 64];
        buf[..n].copy_from_slice(slots);
        let bias = _mm256_set1_epi64x(BIAS);
        let mut acc = [_mm256_setzero_si256(); A];
        let mut ids = [_mm256_setzero_si256(); A];
        for a in 0..A {
            acc[a] = _mm256_add_epi64(_mm256_loadu_si256(buf.as_ptr().add(4 * a) as *const __m256i), bias);
            ids[a] = _mm256_add_epi64(_mm256_set_epi64x(3, 2, 1, 0), _mm256_set1_epi64x(4 * a as i64));
        }
        let mut ovf = _mm256_setzero_si256();
        for it in items {
            let kb = _mm256_set1_epi64x(it.index as i64);
            let vb = _mm256_set1_epi64x(it.v as i64);
            for a in 0..A {
                let sel = _mm256_and_si256(_mm256_cmpeq_epi64(kb, ids[a]), vb);
                let r = _mm256_add_epi64(acc[a], sel);
                ovf = _mm256_or_si256(ovf, _mm256_cmpgt_epi64(acc[a], r)); // biased: signed r < acc
                acc[a] = r;
            }
        }
        if _mm256_testz_si256(ovf, ovf) == 0 {
            sum64_ref(items, slots); // aborts as the scalar fold does; slots still hold the input
            return;
        }
        for a in 0..A {
            _mm256_storeu_si256(
                buf.as_mut_ptr().add(4 * a) as *mut __m256i,
                _mm256_sub_epi64(acc[a], bias),
            );
        }
        slots.copy_from_slice(&buf[..n]);
    }
}

// ---------------------------------------------------------------------------------------------
// Safe entry points: the driver of the lowering. They establish the preconditions and otherwise
// run the scalar reference.

fn usable(n: usize, items: usize) -> bool {
    items != 0 && n != 0 && n <= i32::MAX as usize && has_avx2()
}

pub fn axpy_simd(cx: &Cx, items: &[Xi], slots: &mut [f32]) {
    if usable(slots.len(), items.len()) {
        // Safety: AVX2 detected, 0 < n <= i32::MAX.
        unsafe { axpy_window(cx, items, slots) }
    } else {
        axpy_ref(cx, items, slots)
    }
}

pub fn axpy_sel_simd(cx: &Cx, items: &[Xi], slots: &mut [f32]) {
    if usable(slots.len(), items.len()) {
        // Safety: as above.
        unsafe { axpy_sel_window(cx, items, slots) }
    } else {
        axpy_sel_ref(cx, items, slots)
    }
}

pub fn poly_simd(cx: &Cp, items: &[Xi], slots: &mut [f32]) {
    if usable(slots.len(), items.len()) {
        // Safety: as above.
        unsafe { poly_window(cx, items, slots) }
    } else {
        poly_ref(cx, items, slots)
    }
}

pub fn sum32_simd_window(items: &[Xi], slots: &mut [f32]) {
    if usable(slots.len(), items.len()) {
        // Safety: as above.
        unsafe { sum32_window(&(), items, slots) }
    } else {
        sum32_ref(items, slots)
    }
}

/// Register kernel when the slots fit in at most 8 registers (n <= 64), else the window kernel.
pub fn sum32_simd(items: &[Xi], slots: &mut [f32]) {
    let n = slots.len();
    if !usable(n, items.len()) {
        return sum32_ref(items, slots);
    }
    // Safety: AVX2 detected; n bounds match the const A of each arm.
    unsafe {
        match n {
            1..=8 => sum32_reg::<1>(items, slots),
            9..=16 => sum32_reg::<2>(items, slots),
            17..=32 => sum32_reg::<4>(items, slots),
            33..=64 => sum32_reg::<8>(items, slots),
            _ => sum32_window(&(), items, slots),
        }
    }
}

pub fn sum64_simd_window(items: &[Kv64], slots: &mut [u64]) {
    if usable(slots.len(), items.len()) {
        // Safety: as above.
        unsafe { sum64_window(items, slots) }
    } else {
        sum64_ref(items, slots)
    }
}

/// Register kernel when n <= 16 (A <= 4: the per-item cost grows with A), else the window kernel.
pub fn sum64_simd(items: &[Kv64], slots: &mut [u64]) {
    let n = slots.len();
    if !usable(n, items.len()) {
        return sum64_ref(items, slots);
    }
    // Safety: as above.
    unsafe {
        match n {
            1..=4 => sum64_reg::<1>(items, slots),
            5..=8 => sum64_reg::<2>(items, slots),
            9..=16 => sum64_reg::<4>(items, slots),
            _ => sum64_window(items, slots),
        }
    }
}
