// Family B, sampled member: what the reader sees, what the writer pays, and what happens when
// no buffer is free.

use std::thread;
use std::time::Duration;

use crate::cpu::sync::publish::{Ended, Pressure, SLOTS, Version, published};
use crate::cpu::sync::{Cancel, Pinned};

/// A payload with more than one field, so that a torn read would be visible as two fields of one
/// snapshot disagreeing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Frame {
    count: u64,
    parts: [u64; 32],
}

impl Frame {
    /// Fill the whole snapshot from the producer's private state.
    fn fill(&mut self, count: u64) {
        self.count = count;
        self.parts = [count; 32];
    }

    fn coherent(&self) -> bool {
        self.parts.iter().all(|part| *part == self.count)
    }
}

#[test]
fn nothing_is_published_before_the_first_publication() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    assert!(reader.latest().is_none());
    assert_eq!(reader.version(), Version::NONE);
    writer.draft().fill(1);
    assert!(reader.latest().is_none(), "a draft is not a version");
}

#[test]
fn a_published_version_reaches_a_reader_that_started_before_it() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    writer.draft().fill(7);
    let version = writer.publish().expect("a free buffer");
    assert_eq!(version.count(), 1);
    let seen = reader.latest().expect("the published version");
    assert_eq!(seen.version(), version);
    assert_eq!(seen.count, 7);
    assert!(seen.coherent());
}

#[test]
fn the_version_a_reader_holds_stays_put_while_the_writer_moves_on() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    writer.draft().fill(1);
    writer.publish().unwrap();
    let held: Pinned<'_, Frame> = reader.latest().expect("version 1");
    for count in 2..=3 {
        writer.draft().fill(count);
        writer.publish().unwrap();
    }
    // The reader owns this buffer: no publication can reach it.
    assert_eq!(held.count, 1);
    assert_eq!(held.version().count(), 1);
}

#[test]
fn versions_advance_only_on_a_successful_publication() {
    // Two buffers is the configuration where a reader holding one blocks reuse of the other.
    let (mut writer, mut reader) = published(2, Frame::default);
    writer.draft().fill(1);
    assert_eq!(writer.publish().unwrap().count(), 1);
    assert_eq!(reader.latest().expect("version 1").count, 1);

    // The reader still owns that buffer, and the writer owns its draft: nothing is free.
    writer.draft().fill(2);
    assert_eq!(writer.publish(), Err(Pressure::Full));
    assert_eq!(writer.publish(), Err(Pressure::Full));
    assert_eq!(writer.refused(), 2, "refusals are counted, not versions");
    assert_eq!(
        writer.busy(),
        0,
        "nobody held the metadata: this is capacity"
    );
    assert_eq!(writer.version().count(), 1, "no version was consumed");
    assert_eq!(writer.draft().count, 2, "the draft is untouched");
    assert!(writer.draft().coherent());

    // Giving the buffer back is what unblocks it.
    drop(reader);
    assert_eq!(writer.publish().unwrap().count(), 2);
    assert_eq!(writer.refused(), 2);
}

#[test]
fn a_skipped_version_is_counted_apart_from_a_refused_one() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    for count in 1..=3 {
        writer.draft().fill(count);
        writer.publish().unwrap();
    }
    assert_eq!(writer.refused(), 0);
    assert_eq!(
        writer.overwritten(),
        2,
        "two committed versions went untaken"
    );
    let seen = reader.latest().expect("the newest version");
    assert_eq!(seen.count, 3);
    // The reader compares version numbers to learn what it skipped.
    assert_eq!(seen.version().count(), 3);
}

#[test]
fn three_buffers_never_refuse_a_publication_for_want_of_a_buffer() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    for count in 1..=100 {
        writer.draft().fill(count);
        // Nobody else touches the metadata on this thread, so `Busy` cannot arise either; what
        // three buffers rule out is the capacity refusal, and only that.
        writer.publish().expect("SLOTS buffers never fill up");
        if count % 7 == 0 {
            let seen = reader.latest().expect("a version");
            assert!(seen.coherent());
        }
    }
    assert_eq!(writer.refused(), 0);
    assert_eq!(writer.busy(), 0);
}

#[test]
fn a_contended_publication_reports_pressure_instead_of_waiting() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    writer.draft().fill(11);
    {
        // The reader is descheduled inside a call, holding the metadata. Nothing here sleeps and
        // nothing here waits for it: the writer is told, and keeps everything it had.
        let _busy = reader.hold();
        assert_eq!(writer.publish(), Err(Pressure::Busy));
        assert_eq!(writer.publish(), Err(Pressure::Busy));
    }
    assert_eq!(
        writer.busy(),
        2,
        "contention is counted apart from capacity"
    );
    assert_eq!(writer.refused(), 0, "no buffer was missing");
    assert_eq!(writer.version(), Version::NONE, "no version was consumed");
    assert_eq!(writer.draft().count, 11, "the draft is untouched");
    assert!(writer.draft().coherent());

    // The instant passed; the same draft is published unchanged.
    assert_eq!(writer.publish().expect("the metadata is free").count(), 1);
    let seen = reader.latest().expect("the published version");
    assert_eq!(seen.count, 11);
    assert!(seen.coherent());
}

#[test]
fn a_version_number_is_the_writers_own_and_needs_no_lock() {
    let (mut writer, reader) = published(SLOTS, Frame::default);
    writer.draft().fill(1);
    writer.publish().unwrap();
    // Reporting the committed version must not be able to wait on a reader either.
    let _busy = reader.hold();
    assert_eq!(writer.version().count(), 1);
}

#[test]
fn a_reader_waiting_for_a_version_is_woken_by_the_publication() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    let cancel = Cancel::new();
    thread::scope(|threads| {
        let waiting = threads.spawn(|| {
            let seen = reader.after(Version::NONE, &cancel).expect("a version");
            (seen.version().count(), seen.count)
        });
        thread::sleep(Duration::from_millis(20));
        writer.draft().fill(42);
        writer.publish().unwrap();
        assert_eq!(waiting.join().unwrap(), (1, 42));
    });
}

#[test]
fn a_version_already_in_hand_ends_the_wait_at_once() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    let cancel = Cancel::new();
    writer.draft().fill(5);
    let first = writer.publish().unwrap();
    assert_eq!(reader.latest().unwrap().count, 5);
    // Already taken, and newer than `NONE`: this must not park.
    let seen = reader
        .after(Version::NONE, &cancel)
        .expect("the held version");
    assert_eq!(seen.version(), first);
}

#[test]
fn a_waiting_reader_is_woken_by_a_cancel() {
    let (_writer, mut reader) = published(SLOTS, Frame::default);
    let cancel = Cancel::new();
    thread::scope(|threads| {
        let waiting = threads.spawn(|| reader.after(Version::NONE, &cancel).map(|seen| seen.count));
        thread::sleep(Duration::from_millis(20));
        cancel.raise();
        assert_eq!(waiting.join().unwrap(), Err(Ended::Cancelled));
    });
}

#[test]
fn a_waiting_reader_is_woken_when_the_writer_goes_away() {
    let (writer, mut reader) = published(SLOTS, Frame::default);
    let cancel = Cancel::new();
    thread::scope(|threads| {
        let waiting = threads.spawn(|| reader.after(Version::NONE, &cancel).map(|seen| seen.count));
        thread::sleep(Duration::from_millis(20));
        drop(writer);
        assert_eq!(waiting.join().unwrap(), Err(Ended::Closed));
    });
}

#[test]
fn the_last_version_survives_the_writer() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    let cancel = Cancel::new();
    writer.draft().fill(9);
    writer.publish().unwrap();
    drop(writer);
    let first = {
        let seen = reader.after(Version::NONE, &cancel).expect("version 1");
        assert_eq!(seen.count, 9);
        seen.version()
    };
    // And then there is nothing more to come.
    assert_eq!(
        reader.after(first, &cancel).map(|seen| seen.count),
        Err(Ended::Closed)
    );
}

#[test]
fn a_reader_never_sees_a_half_written_version() {
    let (mut writer, mut reader) = published(SLOTS, Frame::default);
    let cancel = Cancel::new();
    let rounds = 20_000u64;
    thread::scope(|threads| {
        let reading = threads.spawn(|| {
            let mut last = Version::NONE;
            let mut taken = 0u64;
            while last.count() < rounds {
                let Ok(seen) = reader.after(last, &cancel) else {
                    break;
                };
                assert!(
                    seen.coherent(),
                    "torn payload at version {:?}",
                    seen.version()
                );
                assert!(seen.version() > last, "versions must advance");
                assert_eq!(seen.count, seen.version().count());
                last = seen.version();
                taken += 1;
            }
            (last, taken)
        });
        for count in 1..=rounds {
            writer.draft().fill(count);
            // A reader running beside this one holds the metadata now and then, so publishing is
            // a retry loop: `Busy` is a contended instant and never a reason to skip a round
            // here, and the draft survives it untouched.
            loop {
                match writer.publish() {
                    Ok(_) => break,
                    Err(Pressure::Busy) => {
                        assert_eq!(writer.draft().count, count, "the draft survived");
                        thread::yield_now();
                    }
                    Err(Pressure::Full) => panic!("SLOTS buffers never fill up"),
                }
            }
        }
        let (last, taken) = reading.join().unwrap();
        assert_eq!(last.count(), rounds);
        assert!(taken <= rounds, "a version is taken at most once");
        // Sampling is allowed to skip, and what it skipped is accounted for exactly.
        assert_eq!(writer.overwritten() + taken, rounds);
    });
}
