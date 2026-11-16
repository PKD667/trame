//! The item functions and their scalar reference. The scalar item bodies are the specification;
//! `simd.rs` holds the lane version of each.

/// An f32 item: `index` is its key.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xi {
    pub index: u32,
    pub x: f32,
}

/// A u64 item: `index` is its key; `_p` keeps `v` aligned and is always read as part of the key's
/// 64-bit lane (the lane version masks it off).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kv64 {
    pub index: u32,
    pub _p: u32,
    pub v: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Cx {
    pub a: f32,
    pub t: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Cp {
    pub c: [f32; 8],
}

/// Map shape: `slot += a * x`. Two roundings; Rust never contracts `a * x + s` into an fma, so the
/// lane version uses a separate multiply and add.
#[trame::item(key = x.index as usize)]
pub fn axpy(x: &Xi, slot: &mut f32, cx: &Cx) {
    *slot = *slot + cx.a * x.x;
}

/// The same with a branch: `slot += a * x` when `x > t`. The scalar code may mispredict the branch;
/// the lane version is a select, which cannot.
#[trame::item(key = x.index as usize)]
pub fn axpy_sel(x: &Xi, slot: &mut f32, cx: &Cx) {
    if x.x > cx.t {
        *slot = *slot + cx.a * x.x;
    }
}

/// Heavier map shape: Horner polynomial of degree 7 (14 flops), added to the slot.
#[trame::item(key = x.index as usize)]
pub fn poly(x: &Xi, slot: &mut f32, cx: &Cp) {
    let mut p = cx.c[7];
    for i in (0..7).rev() {
        p = p * x.x + cx.c[i];
    }
    *slot = *slot + p;
}

/// Reduce shape, f32: the fold order per key is the whole result.
#[trame::item(key = x.index as usize)]
pub fn sum32(x: &Xi, slot: &mut f32, _cx: &()) {
    *slot = *slot + x.x;
}

/// Reduce shape, u64: the add aborts on overflow (INTERFACE.md: overflow aborts in every build).
#[trame::item(key = x.index as usize)]
pub fn sum64(x: &Kv64, slot: &mut u64, _cx: &()) {
    *slot += x.v;
}

// The scalar references: exactly the expansion of `parallel!(f, &cx, &items, &mut slots)`.
#[inline(never)]
pub fn axpy_ref(cx: &Cx, items: &[Xi], slots: &mut [f32]) {
    trame::__rt::parallel(__trame_key_axpy, axpy, cx, items, slots)
}
#[inline(never)]
pub fn axpy_sel_ref(cx: &Cx, items: &[Xi], slots: &mut [f32]) {
    trame::__rt::parallel(__trame_key_axpy_sel, axpy_sel, cx, items, slots)
}
#[inline(never)]
pub fn poly_ref(cx: &Cp, items: &[Xi], slots: &mut [f32]) {
    trame::__rt::parallel(__trame_key_poly, poly, cx, items, slots)
}
#[inline(never)]
pub fn sum32_ref(items: &[Xi], slots: &mut [f32]) {
    trame::__rt::parallel(__trame_key_sum32, sum32, &(), items, slots)
}
#[inline(never)]
pub fn sum64_ref(items: &[Kv64], slots: &mut [u64]) {
    trame::__rt::parallel(__trame_key_sum64, sum64, &(), items, slots)
}

/// The key check of parallel.rs, first pass, with its message. The lane kernels call it when their
/// own check finds a bad key, so the abort is the scalar abort.
#[cold]
#[inline(never)]
pub fn check_keys_scalar<I>(key: impl Fn(&I) -> usize, items: &[I], n: usize) {
    for item in items {
        let k = key(item);
        assert!(
            k < n,
            "trame: parallel!: an item has key {k}, but there are {n} slots; a key must be below the slot count, as x[i] is. No item ran."
        );
    }
}
