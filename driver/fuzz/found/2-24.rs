// trame-fuzz found: seed 2 index 173
// key: over step std (the shrunk program keeps this difference)
// first seen: seed=2 index=173; shrunk in 9 rounds, 277 candidates
// Expected (R, from the abstract program):
//   [allow-list] item roots=["r0"]
//   [std] item roots=["r0"]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] item calls FF1::into: not listed at this level (driver/lists/A_I.txt) (spec.md, Requirement (Item))
//     --> examples/final.rs:10:13
//      |
//   10 | let _: () = FF1(x).into();
//      |             ^^^^^^^^^^^^^
//      |
//      = note: reached from root r0: r0
//   
//   error: [std] step calls std::thread::functions::spawn: no std (spec.md, Requirement (All levels))
//    --> examples/final.rs:7:9
//     |
//   7 | let _ = std::thread::spawn(|| {});
//     |         ^^^^^^^^^^^^^^^^^^^^^^^^^
//     |
//     = note: reached from root P1::step: P1::step -> f0
//   
//   error: [unsafe] step contains dereference of a raw pointer: no unsafe code (spec.md, Requirement (All levels))
//      --> /nix/store/savfcq79v5hiy9mly85gb0lbx5f0hpz7-rust-minimal-1.97.0-nightly-2026-04-21/lib/rustlib/src/rust/library/core/src/ptr/mod.rs:809:1
//       |
//   809 | / pub const unsafe fn drop_in_place<T: PointeeSized>(to_drop: *mut T)
//   810 | | where
//   811 | |     T: [const] Destruct,
//       | |________________________^
//       |
//       = note: reached from root P1::step: P1::step -> f0 -> core::ptr::drop_in_place
//   
//   error: [std] step calls std::sys::thread::unix::Thread::drop: no std (spec.md, Requirement (All levels))
//      --> /nix/store/savfcq79v5hiy9mly85gb0lbx5f0hpz7-rust-minimal-1.97.0-nightly-2026-04-21/lib/rustlib/src/rust/library/core/src/ptr/mod.rs:809:1
//       |
//   809 | / pub const unsafe fn drop_in_place<T: PointeeSized>(to_drop: *mut T)
//   810 | | where
//   811 | |     T: [const] Destruct,
//       | |________________________^
//       |
//       = note: reached from root P1::step: P1::step -> f0 -> core::ptr::drop_in_place -> core::ptr::drop_in_place
//   
//   error: [unsafe] step contains dereference of a raw pointer: no unsafe code (spec.md, Requirement (All levels))
//      --> /nix/store/savfcq79v5hiy9mly85gb0lbx5f0hpz7-rust-minimal-1.97.0-nightly-2026-04-21/lib/rustlib/src/rust/library/core/src/ptr/mod.rs:809:1
//       |
//   809 | / pub const unsafe fn drop_in_place<T: PointeeSized>(to_drop: *mut T)
//   810 | | where
//   811 | |     T: [const] Destruct,
//       | |________________________^
//       |
//       = note: reached from root P1::step: P1::step -> f0 -> core::ptr::drop_in_place -> core::ptr::drop_in_place
//   
//   error: [allow-list] step calls alloc::sync::Arc::drop: not listed at this level (driver/lists/A_C.txt) (spec.md, Requirement (Step))
//      --> /nix/store/savfcq79v5hiy9mly85gb0lbx5f0hpz7-rust-minimal-1.97.0-nightly-2026-04-21/lib/rustlib/src/rust/library/core/src/ptr/mod.rs:809:1
//       |
//   809 | / pub const unsafe fn drop_in_place<T: PointeeSized>(to_drop: *mut T)
//   810 | | where
//   811 | |     T: [const] Destruct,
//       | |________________________^
//       |
//       = note: reached from root P1::step: P1::step -> f0 -> core::ptr::drop_in_place -> core::ptr::drop_in_place
//   
//   error: [unsafe] step contains dereference of a raw pointer: no unsafe code (spec.md, Requirement (All levels))
//      --> /nix/store/savfcq79v5hiy9mly85gb0lbx5f0hpz7-rust-minimal-1.97.0-nightly-2026-04-21/lib/rustlib/src/rust/library/core/src/ptr/mod.rs:809:1
//       |
//   809 | / pub const unsafe fn drop_in_place<T: PointeeSized>(to_drop: *mut T)
//   810 | | where
//   811 | |     T: [const] Destruct,
//       | |________________________^
//       |
//       = note: reached from root P1::step: P1::step -> f0 -> core::ptr::drop_in_place -> core::ptr::drop_in_place -> core::ptr::drop_in_place
//   
//   error: [allow-list] step calls alloc::sync::Arc::drop: not listed at this level (driver/lists/A_C.txt) (spec.md, Requirement (Step))
//      --> /nix/store/savfcq79v5hiy9mly85gb0lbx5f0hpz7-rust-minimal-1.97.0-nightly-2026-04-21/lib/rustlib/src/rust/library/core/src/ptr/mod.rs:809:1
//       |
//   809 | / pub const unsafe fn drop_in_place<T: PointeeSized>(to_drop: *mut T)
//   810 | | where
//   811 | |     T: [const] Destruct,
//       | |________________________^
//       |
//       = note: reached from root P1::step: P1::step -> f0 -> core::ptr::drop_in_place -> core::ptr::drop_in_place -> core::ptr::drop_in_place -> core::ptr::drop_in_place
//   
//   error: [unsafe] step contains dereference of a raw pointer: no unsafe code (spec.md, Requirement (All levels))
//      --> /nix/store/savfcq79v5hiy9mly85gb0lbx5f0hpz7-rust-minimal-1.97.0-nightly-2026-04-21/lib/rustlib/src/rust/library/core/src/ptr/mod.rs:809:1
//       |
//   809 | / pub const unsafe fn drop_in_place<T: PointeeSized>(to_drop: *mut T)
//   810 | | where
//   811 | |     T: [const] Destruct,
//       | |________________________^
//       |
//       = note: reached from root P1::step: P1::step -> f0 -> core::ptr::drop_in_place -> core::ptr::drop_in_place -> core::ptr::drop_in_place -> core::ptr::drop_in_place
//   
//   error: could not compile `batch` (example "final") due to 9 previous errors
// Differences (shrunk program): ATTR-MISS [std] item (root r0); OVER-REFUSAL [std] step (root P1::step)
// Differences (first seen): ATTR-MISS [allocation] item (root r1); ATTR-MISS [counted-loop] item (root r1); ATTR-MISS [deny] item (root r1); ATTR-MISS [effects] item (root r1); ATTR-MISS [indirect] item (root r1); ATTR-MISS [recursion] item (root any root); ATTR-MISS [std] item (root r1); OVER-REFUSAL [indirect] step (root P2::step); OVER-REFUSAL [std] step (root P2::step); OVER-REFUSAL [counted-loop] step (root P2::step); OVER-REFUSAL [recursion] step (no root named)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=2 index=173 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]
static GLOB: u32 = 3;
fn nop0(_y: u32) {}
fn nop1(_y: u32) {}
fn f0(x: u32) {
let _ = std::thread::spawn(|| {});
}
fn r0(x: u32) {
let _: () = FF1(x).into();
}
pub struct P1 { pub x: u32 }
impl P1 { pub fn step(&mut self) {
let x = self.x;
f0(x);
} }
pub struct FF1(u32);
impl From<FF1> for () { fn from(f: FF1) {
let x = f.0;
f0(x);
} }
fn main() {
    trame::__check::item(&r0);
    let p1 = P1 { x: 1 };
    trame::__check::step(&p1);
}
