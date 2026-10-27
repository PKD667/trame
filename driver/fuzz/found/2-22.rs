// trame-fuzz found: seed 2 index 173
// key: over step indirect (the shrunk program keeps this difference)
// first seen: seed=2 index=173; shrunk in 9 rounds, 264 candidates
// Expected (R, from the abstract program):
//   [allow-list] item roots=["r0"]
//   [indirect] item roots=["r0"]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] item calls FF1::into: not listed at this level (driver/lists/A_I.txt) (spec.md, Requirement (Item))
//     --> examples/final.rs:10:13
//      |
//   10 | let _: () = FF1(x).into();
//      |             ^^^^^^^^^^^^^
//      |
//      = note: reached from root r0: r0
//   
//   error: [indirect] step call of a trait object method: no indirect calls (spec.md, Requirement (All levels))
//    --> examples/final.rs:7:81
//     |
//   7 | { let d: &dyn Fn(u32) = if x & 1 == 0 { &|_y: u32| {} } else { &|_y: u32| {} }; d(x); }
//     |                                                                                 ^^^^
//     |
//     = note: reached from root P1::step: P1::step -> f0
//   
//   error: could not compile `batch` (example "final") due to 2 previous errors
// Differences (shrunk program): ATTR-MISS [indirect] item (root r0); OVER-REFUSAL [indirect] step (root P1::step)
// Differences (first seen): ATTR-MISS [allocation] item (root r1); ATTR-MISS [counted-loop] item (root r1); ATTR-MISS [deny] item (root r1); ATTR-MISS [effects] item (root r1); ATTR-MISS [indirect] item (root r1); ATTR-MISS [recursion] item (root any root); ATTR-MISS [std] item (root r1); OVER-REFUSAL [indirect] step (root P2::step); OVER-REFUSAL [std] step (root P2::step); OVER-REFUSAL [counted-loop] step (root P2::step); OVER-REFUSAL [recursion] step (no root named)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=2 index=173 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]
static GLOB: u32 = 3;
fn nop0(_y: u32) {}
fn nop1(_y: u32) {}
fn f0(x: u32) {
{ let d: &dyn Fn(u32) = if x & 1 == 0 { &|_y: u32| {} } else { &|_y: u32| {} }; d(x); }
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
