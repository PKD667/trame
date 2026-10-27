// trame-fuzz found: seed 1 index 11
// key: attr-miss recursion item (the shrunk program keeps this difference)
// first seen: seed=1 index=11; shrunk in 8 rounds, 291 candidates
// Expected (R, from the abstract program):
//   [allow-list] item roots=["r0"]
//   [recursion] item roots=[]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] item calls FF1::into: not listed at this level (driver/lists/A_I.txt) (spec.md, Requirement (Item))
//    --> examples/final.rs:7:13
//     |
//   7 | let _: () = FF1(x).into();
//     |             ^^^^^^^^^^^^^
//     |
//     = note: reached from root r0: r0 -> f0
//   
//   error: could not compile `batch` (example "final") due to 1 previous error
// Differences (shrunk program): ATTR-MISS [recursion] item (root any root)
// Differences (first seen): ATTR-MISS [recursion] item (root any root)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=1 index=11 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]
static GLOB: u32 = 3;
fn nop0(_y: u32) {}
fn nop1(_y: u32) {}
fn f0(x: u32) {
let _: () = FF1(x).into();
}
fn r0(x: u32) {
f0(x);
}
pub struct FF1(u32);
impl From<FF1> for () { fn from(f: FF1) {
let x = f.0;
f0(x);
} }
fn main() {
    trame::__check::item(&r0);
}
