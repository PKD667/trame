// Family C: the re-exports are the standard atomics, with the ordering named at each call.
//
// There is no wrapper to test, so what these check is that the family is usable as the
// application already uses it — the same operations with the same orderings `src/nerve/shared.rs`
// chose — and that a release publication paired with an acquire load carries the payload it is
// supposed to carry.

use std::thread;

use crate::cpu::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::*};

#[test]
fn the_conventional_operations_are_all_here() {
    let exact = AtomicU64::new(0);
    exact.fetch_add(3, Release);
    assert_eq!(exact.load(Acquire), 3);
    assert_eq!(
        exact.swap(0, Release),
        3,
        "take reads and zeroes in one step"
    );
    assert_eq!(
        exact.compare_exchange(0, 9, AcqRel, Acquire),
        Ok(0),
        "a successful exchange returns what was there"
    );
    assert_eq!(
        exact.compare_exchange(0, 11, AcqRel, Acquire),
        Err(9),
        "a failed exchange returns what it found instead"
    );
    // The narrow widths are the same operations, and the family offers exactly these four.
    let flag = AtomicU32::new(0);
    flag.store(1, Release);
    assert_eq!(flag.load(Acquire), 1);
    let signed = AtomicI32::new(-1);
    signed.fetch_add(1, Relaxed);
    assert_eq!(signed.load(Relaxed), 0);
}

#[test]
fn a_release_flag_publishes_the_writes_before_it() {
    // The chain: write, release-publish, observe, acquire, read. What it establishes is
    // between threads of this process and nothing else; no ordering here reaches another rank.
    let payload = AtomicU64::new(0);
    let ready = AtomicU32::new(0);
    thread::scope(|threads| {
        threads.spawn(|| {
            payload.store(0xfeed, Relaxed);
            ready.store(1, Release);
        });
        threads.spawn(|| {
            while ready.load(Acquire) == 0 {
                thread::yield_now();
            }
            assert_eq!(payload.load(Relaxed), 0xfeed);
        });
    });
}
