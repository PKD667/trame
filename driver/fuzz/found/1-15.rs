// trame-fuzz found: seed 1 index 71
// key: attr-miss static worker (the shrunk program keeps this difference)
// first seen: seed=1 index=71; shrunk in 14 rounds, 368 candidates
// Expected (R, from the abstract program):
//   [allow-list] worker roots=["r0"]
//   [static] worker roots=["r0"]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] worker calls FF1::into: not listed at this level (driver/lists/A_W.txt) (spec.md, Requirement (Worker))
//    --> examples/final.rs:7:13
//     |
//   7 | let _: () = FF1(x).into();
//     |             ^^^^^^^^^^^^^
//     |
//     = note: reached from root r0: r0
//   
//   error: could not compile `batch` (example "final") due to 1 previous error
// Differences (shrunk program): ATTR-MISS [static] worker (root r0)
// Differences (first seen): ATTR-MISS [static] worker (root r0)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=1 index=71 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]
static GLOB: u32 = 3;
fn nop0(_y: u32) {}
fn nop1(_y: u32) {}
fn r0(x: u32) {
let _: () = FF1(x).into();
}
pub struct FF1(u32);
impl From<FF1> for () { fn from(f: FF1) {
let x = f.0;
let _ = GLOB + x;
} }
fn main() {
    trame::__check::worker(&r0);
}
