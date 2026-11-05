//! AVX-512 lane sub-backend (x86-64-v4: AVX-512 F, BW, DQ, VL).
//!
//! Stub: not yet implemented.

use core::arch::x86_64::*;
use super::{Lane, CmpOp};

/// Check if the CPU supports AVX-512 (F, BW, DQ, VL) (used in the launch path only).
/// Uses core::arch::x86_64::__cpuid for feature detection in no_std contexts.
#[doc(hidden)]
pub fn has_avx512() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        // AVX-512 F is CPUID.7:EBX bit 16
        // AVX-512 BW is CPUID.7:EBX bit 30
        let cpuid_7 = __cpuid_count(7, 0);
        let has_f = (cpuid_7.ebx & (1 << 16)) != 0;
        let has_bw = (cpuid_7.ebx & (1 << 30)) != 0;
        has_f && has_bw
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}
