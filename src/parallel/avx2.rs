//! AVX2 lane sub-backend (x86-64-v3: AVX2 and FMA).
//!
//! Uses core::arch::x86_64 (stable) instead of std::simd (unstable on rustc 1.98.1, issue 86656).

use core::arch::x86_64::*;

// The macro is_x86_feature_detected! is provided implicitly in scope for x86_64 target.
// We need to make it available: it's not strictly imported but is a compiler-provided macro.
use super::lane::{CmpOp, Lane};

/// AVX2 backend: 8 lanes of f32 or 4 lanes of f64 or f32 or other combinations per kernel.
/// Kernels are selected per item and per shape (map vs reduce, distinct vs contiguous keys).
#[doc(hidden)]
pub struct Avx2F32;

impl Lane for Avx2F32 {
    type Vec = __m256;
    type Mask = __m256;

    unsafe fn store(v: Self::Vec, ptr: *mut u8, offset: usize) {
        unsafe {
            let p = (ptr as *mut f32).add(offset);
            _mm256_storeu_ps(p, v);
        }
    }

    unsafe fn load_slot(ptr: *const u8, offset: usize) -> Self::Vec {
        unsafe {
            let p = (ptr as *const f32).add(offset);
            _mm256_loadu_ps(p)
        }
    }

    unsafe fn gather_slot(ptr: *const u8, keys: &[u32], _key_count: usize) -> Self::Vec {
        // Use gather instruction (requires indices as i32 vector)
        let mut indices = [0i32; 8];
        for i in 0..keys.len().min(8) {
            indices[i] = keys[i] as i32;
        }
        unsafe {
            let idx_vec = _mm256_loadu_si256(indices.as_ptr() as *const __m256i);
            _mm256_i32gather_ps(ptr as *const f32, idx_vec, 4)
        }
    }

    fn select(mask: Self::Mask, a: Self::Vec, b: Self::Vec) -> Self::Vec {
        unsafe { _mm256_blendv_ps(b, a, mask) }
    }

    fn splat(s: Self::Vec) -> Self::Vec {
        s // Already scalar broadcasted by the macro
    }

    fn cmp(_a: Self::Vec, _b: Self::Vec, _op: CmpOp) -> Self::Mask {
        // Stub: not used in the current scope
        unsafe { _mm256_setzero_ps() }
    }

    fn add(a: Self::Vec, b: Self::Vec) -> Self::Vec {
        unsafe { _mm256_add_ps(a, b) }
    }

    fn sub(a: Self::Vec, b: Self::Vec) -> Self::Vec {
        unsafe { _mm256_sub_ps(a, b) }
    }

    fn mul(a: Self::Vec, b: Self::Vec) -> Self::Vec {
        unsafe { _mm256_mul_ps(a, b) }
    }

    fn div(a: Self::Vec, b: Self::Vec) -> Self::Vec {
        unsafe { _mm256_div_ps(a, b) }
    }

    fn neg(a: Self::Vec) -> Self::Vec {
        unsafe { _mm256_xor_ps(a, _mm256_set1_ps(-0.0)) }
    }
}

/// Check if the CPU supports AVX2 and is used in the launch path only.
/// Uses core::arch::x86_64::__cpuid for feature detection in no_std contexts.
#[doc(hidden)]
pub fn has_avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        // AVX2 is CPUID.7:EBX bit 5
        let cpuid_7 = __cpuid_count(7, 0);
        (cpuid_7.ebx & (1 << 5)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}
