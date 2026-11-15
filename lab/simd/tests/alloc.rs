//! The lane kernels allocate nothing, the first call included (parallel.rs allocates nothing; the
//! lowering must not change that). The counter is per thread, as in tests/alloc.rs of trame.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use trame_simd_experiment::simd::*;
use trame_simd_experiment::*;

struct Counting;
thread_local! { static ALLOCS: Cell<u64> = const { Cell::new(0) }; }
fn count() -> u64 {
    ALLOCS.with(Cell::get)
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn kernels_never_allocate() {
    let c0 = count();
    drop(std::hint::black_box(Box::new(1u64)));
    assert!(count() > c0, "the counter works");
    let ns = [1usize, 5, 8, 9, 33, 64];
    let sets: Vec<(usize, Vec<Xi>, Vec<Kv64>)> = ns
        .iter()
        .map(|&n| {
            (
                n,
                (0..1000u32).map(|i| Xi { index: (i * 7) % n as u32, x: i as f32 }).collect(),
                (0..1000u32).map(|i| Kv64 { index: (i * 7) % n as u32, _p: 0, v: i as u64 }).collect(),
            )
        })
        .collect();
    let mut s32 = vec![0f32; 64];
    let mut s64 = vec![0u64; 64];
    let cx = Cx { a: 1.5, t: 0.0 };
    let cp = Cp { c: [1.0; 8] };
    // The first call of each kernel is counted too (no warm-up): detection of AVX2 is in it.
    let at = count();
    for (n, items, items64) in &sets {
        let n = *n;
        axpy_simd(&cx, items, &mut s32[..n]);
        axpy_sel_simd(&cx, items, &mut s32[..n]);
        poly_simd(&cp, items, &mut s32[..n]);
        sum32_simd_window(items, &mut s32[..n]);
        sum32_simd(items, &mut s32[..n]);
        sum64_simd_window(items64, &mut s64[..n]);
        sum64_simd(items64, &mut s64[..n]);
    }
    assert_eq!(count(), at, "a kernel allocated");
}
