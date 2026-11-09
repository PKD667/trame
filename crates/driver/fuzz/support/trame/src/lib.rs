//! Stand-in for trame, for the fuzzer's batch crates (crates/driver/fuzz/README.md). `__check` is the real
//! `check.rs`, included by path. Effects have the paths in crates/driver/lists/effects.txt.
#[path = "../../../../../../src/check.rs"]
pub mod __check;

pub mod io {
    pub fn send(_x: u32) {}
    pub fn recv() -> u32 {
        0
    }
}
pub mod time {
    pub fn now() -> u64 {
        0
    }
}
pub mod __rt {
    pub fn parallel() {}
}
pub mod math {
    pub mod f32 {
        pub fn exp(x: f32) -> f32 {
            x
        }
    }
}
