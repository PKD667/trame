// trame-fuzz found: seed 3 index 138
// key: attr-miss effects step (the shrunk program keeps this difference)
// first seen: seed=3 index=138; shrunk in 17 rounds, 979 candidates
// Expected (R, from the abstract program):
//   [allow-list] step roots=["P0::step"]
//   [effects] step roots=["P0::step"]
// Actual (driver, from `cargo trame check --examples --keep-going`):
//   error: [allow-list] step calls FF1::into: not listed at this level (driver/lists/A_C.txt) (spec.md, Requirement (Step))
//    --> examples/final.rs:9:13
//     |
//   9 | let _: () = FF1(x).into();
//     |             ^^^^^^^^^^^^^
//     |
//     = note: reached from root P0::step: P0::step
//   
//   error: could not compile `batch` (example "final") due to 1 previous error
// Differences (shrunk program): ATTR-MISS [effects] step (root P0::step)
// Differences (first seen): ATTR-MISS [effects] step (root P0::step); ATTR-MISS [indirect] step (root P0::step)
// Decide whether R or V is wrong by re-reading spec.md section 3 and driver/lists/.
// Requires driver/fuzz/support/trame (the stand-in crate) as a path dependency.

// trame-fuzz: seed=3 index=138 (driver/fuzz/README.md). Generated; the expectation is in expect.jsonl.
#![allow(dead_code, unused, non_camel_case_types, non_upper_case_globals, unused_must_use)]
static GLOB: u32 = 3;
fn nop0(_y: u32) {}
fn nop1(_y: u32) {}
pub struct P0 { pub x: u32 }
impl P0 { pub fn step(&mut self) {
let x = self.x;
let _: () = FF1(x).into();
} }
pub struct FF1(u32);
impl From<FF1> for () { fn from(f: FF1) {
let x = f.0;
trame::io::barrier();
} }
fn main() {
    let p0 = P0 { x: 1 };
    trame::__check::step(&p0);
}
