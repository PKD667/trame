// trame-fuzz found: seed 2 index 173
// key: attr-miss allocation item (the shrunk program keeps this difference)
// first seen: seed=2 index=173; shrunk in 9 rounds, 252 candidates
// Expected (R, from the abstract program):
//   [allocation] item roots=["r0"]
//   [allow-list] item roots=["r0"]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] item calls FF1::into: not listed at this level (driver/lists/A_I.txt) (spec.md, Requirement (Item))
//     --> examples/final.rs:10:13
//      |
//   10 | let _: () = FF1(x).into();
//      |             ^^^^^^^^^^^^^
//      |
//      = note: reached from root r0: r0
//   
//   error: could not compile `batch` (example "final") due to 1 previous error
// Differences (shrunk program): ATTR-MISS [allocation] item (root r0)
// Differences (first seen): ATTR-MISS [allocation] item (root r1); ATTR-MISS [counted-loop] item (root r1); ATTR-MISS [deny] item (root r1); ATTR-MISS [effects] item (root r1); ATTR-MISS [indirect] item (root r1); ATTR-MISS [recursion] item (root any root); ATTR-MISS [std] item (root r1); OVER-REFUSAL [indirect] step (root P2::step); OVER-REFUSAL [std] step (root P2::step); OVER-REFUSAL [counted-loop] step (root P2::step); OVER-REFUSAL [recursion] step (no root named)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=2 index=173 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]
static GLOB: u32 = 3;
fn nop0(_y: u32) {}
fn nop1(_y: u32) {}
fn f0(x: u32) {
{ let mut v: Vec<u32> = Vec::new(); v.push(x); }
}
fn r0(x: u32) {
let _: () = FF1(x).into();
}
pub struct FF1(u32);
impl From<FF1> for () { fn from(f: FF1) {
let x = f.0;
f0(x);
} }
fn main() {
    trame::__check::item(&r0);
}
