//! Lane sub-backends: the type and dispatch for parallel! lowering to explicit SIMD.

/// A lane sub-backend: one vector type, load/store and operations.
/// 
/// The scalar `parallel!(f, &cx, &items, &mut slots)` in parallel.rs is the reference.
/// A lane impl runs kernels that answer the same slots via a schedule of SIMD operations,
/// with the same exactness rules. It has no allocation; the kernels use stack only.
#[doc(hidden)]
pub trait Lane: Sized {
    /// The vector type: T x W (W lanes of type T).
    type Vec;
    /// The mask type for comparisons (true lanes, false lanes).
    type Mask;
    
    /// Load W items from consecutive memory, splitting into keys and values (generic over item type).
    /// The item is `repr(C)` without padding.
    // Load is item-dependent and must be inlined in the kernels; it's not part of the trait.
    
    /// Store W lanes back to consecutive memory.
    unsafe fn store(v: Self::Vec, ptr: *mut u8, offset: usize);
    
    /// Load W slots from ptr[offset * sizeof(S)].
    unsafe fn load_slot(ptr: *const u8, offset: usize) -> Self::Vec;
    
    /// Gather: load slots at indices held in a vector (key vector).
    unsafe fn gather_slot(ptr: *const u8, keys: &[u32], key_count: usize) -> Self::Vec;
    
    /// Lane select: choose from `a` or `b` per lane based on `mask`.
    fn select(mask: Self::Mask, a: Self::Vec, b: Self::Vec) -> Self::Vec;
    
    /// Splat a scalar to all lanes.
    fn splat(s: Self::Vec) -> Self::Vec;
    
    /// Compare (==, <, >, etc.) producing a mask.
    fn cmp(a: Self::Vec, b: Self::Vec, op: CmpOp) -> Self::Mask;
    
    /// Arithmetic operations.
    fn add(a: Self::Vec, b: Self::Vec) -> Self::Vec;
    fn sub(a: Self::Vec, b: Self::Vec) -> Self::Vec;
    fn mul(a: Self::Vec, b: Self::Vec) -> Self::Vec;
    fn div(a: Self::Vec, b: Self::Vec) -> Self::Vec;
    fn neg(a: Self::Vec) -> Self::Vec;
}

/// Comparison operators for lane code.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[cfg(feature = "lanes-avx2")]
#[path = "avx2.rs"]
pub mod avx2;

#[cfg(feature = "lanes-avx512")]
#[path = "avx512.rs"]
pub mod avx512;
