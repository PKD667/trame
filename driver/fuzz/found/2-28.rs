// trame-fuzz found: seed 2 index 192
// key: attr-miss unsafe worker (the shrunk program keeps this difference)
// first seen: seed=2 index=192; shrunk in 9 rounds, 262 candidates
// Expected (R, from the abstract program):
//   [allow-list] worker roots=["r0"]
//   [std] worker roots=["r0"]
//   [unsafe] worker roots=["r0"]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] worker calls FF1::into: not listed at this level (driver/lists/A_W.txt) (spec.md, Requirement (Worker))
//    --> examples/final.rs:7:13
//     |
//   7 | let _: () = FF1(x).into();
//     |             ^^^^^^^^^^^^^
//     |
//     = note: reached from root r0: r0 -> f0
//   
//   error: could not compile `batch` (example "final") due to 1 previous error
// Differences (shrunk program): ATTR-MISS [std] worker (root r0); ATTR-MISS [unsafe] worker (root r0)
// Differences (first seen): ATTR-MISS [std] worker (root r1|r0); ATTR-MISS [unsafe] worker (root r1|r0)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=2 index=192 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
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
println!("{}", x);
} }
fn main() {
    trame::__check::worker(&r0);
}
