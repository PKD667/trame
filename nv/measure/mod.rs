//! The resident-device evidence below the contract (`backend.md` §10): a round trip over the rings,
//! and the transport's refusals.
//!
//! They name the ring machinery, which is not a surface, so they live inside the crate. A
//! measurement is a kernel plus the host program that launches it, and cuda-oxide embeds a kernel
//! in the executable of the crate that holds it; `cfg(test)` is what keeps these kernels in the
//! crate's own test executable and out of the library every `cuda` build links. Each is an ignored
//! test so that an ordinary `cargo test` never asks for a GPU.
//!
//! Run, from the workspace root on a GPU host prepared by `trame/nv/build/tests/prep.sh`:
//!
//! ```text
//! TRAME_MEASURE='<arguments>' cargo nv test -p trame --features cuda --lib -- \
//!     --ignored --exact --nocapture nv::measure::pingpong
//! cargo nv test -p trame --features cuda --lib -- --ignored --exact --nocapture nv::measure::cases
//! ```
//!
//! libtest owns the executable's argv, so a measurement's own arguments come from `TRAME_MEASURE`,
//! split on whitespace, with the grammar they had as an executable's argv. Unset is no arguments.

use std::env::{self, VarError};

mod cases;
mod pingpong;

/// The measurement's arguments, as `env::args().skip(1)` gave them to it as an executable.
fn arguments() -> std::vec::IntoIter<String> {
    let words = match env::var("TRAME_MEASURE") {
        Ok(words) => words,
        Err(VarError::NotPresent) => String::new(),
        Err(VarError::NotUnicode(words)) => panic!("TRAME_MEASURE is not unicode: {words:?}"),
    };
    words
        .split_whitespace()
        .map(String::from)
        .collect::<Vec<_>>()
        .into_iter()
}

#[test]
#[ignore = "needs a GPU and the cuda-oxide backend: run through `cargo nv test`"]
fn pingpong() {
    pingpong::main();
}

#[test]
#[ignore = "needs a GPU and the cuda-oxide backend: run through `cargo nv test`"]
fn cases() {
    cases::main();
}
