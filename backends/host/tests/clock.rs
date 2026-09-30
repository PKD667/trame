// What `clock` promises: monotonic readings that share one process origin. What it does not
// promise, and does not need to: agreement with any other process, on this host or another. That
// is what a monotonic reading is for and why a wall clock is not one.

use crate::contract::Span;
use std::time::Duration;

use crate::host::clock;

#[test]
fn readings_never_go_backwards() {
    let first = clock::reading();
    let second = clock::reading();
    assert!(second.elapsed() >= first.elapsed());
    // A reading earlier than the one it is subtracted from is zero, not a wrap.
    assert_eq!(first.since(second), Ok(Span::default()));
}

#[test]
fn a_reading_measures_the_span_that_passed() {
    let before = clock::reading();
    std::thread::sleep(Duration::from_millis(5));
    let after = clock::reading();
    assert!(after.since(before).expect("one clock") >= Span::from_millis(5));
}

#[test]
fn every_reading_in_this_process_counts_from_one_origin() {
    // Two readings taken far apart differ by the span between them, which only holds if they
    // share a base. A per-call origin would make both of them nearly zero.
    let first = clock::reading();
    std::thread::sleep(Duration::from_millis(2));
    let second = clock::reading();
    assert!(second.elapsed() > first.elapsed());
    assert_eq!(
        second.since(first),
        Ok(Span::from_nanos(
            second.elapsed().nanos() - first.elapsed().nanos()
        ))
    );
}

#[test]
fn nothing_here_claims_two_processes_agree() {
    // Monotonic readings are per process by construction: this one carries no absolute value to
    // compare with, which is the whole of the guarantee. A value two hosts could compare would
    // have to be a wall clock, and that is the host's to provide rather than this backend's.
    let mine = clock::reading();
    // Monotonic readings are per process by construction, so the only claim is that this
    // process's origin is recent: under a year old, which no restart makes false.
    assert!(mine.elapsed() < Span::from_millis(1000 * 60 * 60 * 24 * 365));
}
