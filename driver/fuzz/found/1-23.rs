// trame-fuzz found: seed 1 index 124
// key: attr-miss deny step (the shrunk program keeps this difference)
// first seen: seed=1 index=124; shrunk in 12 rounds, 256 candidates
// Expected (R, from the abstract program):
//   [allow-list] step roots=["P0::step"]
//   [deny] step roots=["P0::step"]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] step calls FF1::into: not listed at this level (driver/lists/A_C.txt) (spec.md, Requirement (Step))
//    --> examples/final.rs:7:13
//     |
//   7 | let _: () = FF1(x).into();
//     |             ^^^^^^^^^^^^^
//     |
//     = note: reached from root P0::step: P0::step -> f0
//   
//   error: could not compile `batch` (example "final") due to 1 previous error
// Differences (shrunk program): ATTR-MISS [deny] step (root P0::step)
// Differences (first seen): ATTR-MISS [deny] step (root P0::step); ATTR-MISS [effects] step (root P0::step)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=1 index=124 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]
static GLOB: u32 = 3;
fn nop0(_y: u32) {}
fn nop1(_y: u32) {}
fn f0(x: u32) {
let _: () = FF1(x).into();
}
pub struct P0 { pub x: u32 }
impl P0 { pub fn step(&mut self) {
let x = self.x;
f0(x);
} }
pub struct FF1(u32);
impl From<FF1> for () { fn from(f: FF1) {
let x = f.0;
(x..).for_each(|_i: u32| {});
} }
fn main() {
    let p0 = P0 { x: 1 };
    trame::__check::step(&p0);
}
