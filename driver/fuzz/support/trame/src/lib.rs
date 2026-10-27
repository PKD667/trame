//! Stand-in for trame, for the fuzzer's batch crates (driver/fuzz/README.md). `__check` is the real
//! `check.rs`, included by path. Effects have the paths in driver/lists/effects.txt.
#[path = "../../../../../check.rs"]
pub mod __check;

pub mod io {
    pub fn send(_x: u32) {}
    pub fn recv() -> u32 {
        0
    }
    pub fn barrier() {}
}
pub mod time {
    pub fn now() -> u64 {
        0
    }
}
pub mod concurrent {
    pub fn concurrent() {}
}
pub mod __rt {
    pub fn parallel() {}
}
pub mod math {
    pub fn exp(x: f32) -> f32 {
        x
    }
}
