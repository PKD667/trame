#![no_std]
extern crate alloc;

// Simulate the CPU check
fn main() {
    #[cfg(all(target_arch = "x86_64", feature = "lanes-avx2"))]
    {
        let cpuid_7 = unsafe { core::arch::x86_64::__cpuid_count(7, 0) };
        let has_avx2 = (cpuid_7.ebx & (1 << 5)) != 0;
        println!("AVX2 supported: {}", has_avx2);
    }
}
