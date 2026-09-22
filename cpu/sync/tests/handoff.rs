// Family B, reliable member: what moves, what comes back, what a full queue does with a payload
// it will not take, and what each side learns when the other goes away.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::thread;

use crate::cpu::sync::handoff::{Idle, Refused, Unsent, handoff, stocked};

/// A block, as the log uses one: storage the producer fills and the consumer empties and returns.
fn block() -> Vec<u8> {
    Vec::with_capacity(64)
}

/// A payload that says when it was dropped, so "accepted" and "delivered" can be told apart.
#[derive(Debug)]
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, SeqCst);
    }
}

#[test]
fn a_payload_moves_to_the_consumer_and_the_storage_comes_back() {
    let (mut sender, mut receiver) = stocked(2, block);
    assert_eq!(receiver.spares(), 2);

    let mut filling = sender.spare().expect("a reserved block");
    filling.extend_from_slice(b"record");
    sender.send(filling).expect("room for one payload");
    assert_eq!(sender.pending(), 1);
    assert_eq!(receiver.spares(), 1, "the filled block left the spare pool");

    let mut taken = receiver.recv().expect("the payload");
    assert_eq!(taken.as_slice(), b"record");
    taken.clear();
    assert!(receiver.give(taken).is_ok(), "the pool had room");
    assert_eq!(receiver.spares(), 2);
    assert_eq!(sender.pending(), 0);
    assert_eq!(receiver.recv().unwrap_err(), Idle::Empty);
}

#[test]
fn a_full_queue_gives_the_payload_back_instead_of_dropping_it() {
    let (mut sender, mut receiver) = handoff(2);
    sender.send(String::from("one")).unwrap();
    sender.send(String::from("two")).unwrap();

    let refused = sender
        .send(String::from("three"))
        .expect_err("a third payload");
    assert_eq!(refused.why, Refused::Full);
    // The caller still owns it: trace drops it and counts, control fails explicitly.
    assert_eq!(refused.value, "three");
    assert_eq!(sender.pending(), 2);

    // Nothing that was accepted was disturbed by the refusal.
    assert_eq!(receiver.recv().unwrap(), "one");
    assert_eq!(receiver.recv().unwrap(), "two");
    assert_eq!(receiver.recv().unwrap_err(), Idle::Empty);
    // And the freed room is usable again.
    sender.send(refused.into_inner()).unwrap();
    assert_eq!(receiver.recv().unwrap(), "three");
}

#[test]
fn storage_beyond_the_reserved_depth_is_handed_back_not_dropped() {
    let (mut sender, mut receiver) = handoff::<Vec<u8>>(1);
    assert!(receiver.give(block()).is_ok());
    let extra = receiver.give(block()).expect_err("the pool is one deep");
    assert_eq!(extra.why, Refused::Full);
    assert_eq!(extra.value.capacity(), 64);
    assert_eq!(receiver.spares(), 1);
    assert!(sender.spare().is_ok());
    assert_eq!(sender.spare().unwrap_err(), Idle::Empty);
}

#[test]
fn a_contended_submission_keeps_its_payload_instead_of_waiting() {
    let (mut sender, mut receiver) = stocked(2, block);
    let mut record = sender.spare().expect("a reserved block");
    record.extend_from_slice(b"held");
    {
        // The consumer is descheduled inside a call, holding the metadata. Nothing below sleeps,
        // and nothing below waits for it.
        let _busy = receiver.hold();
        let refused = sender.send(record).expect_err("the metadata was held");
        assert_eq!(refused.why, Refused::Busy);
        assert_eq!(refused.value.as_slice(), b"held", "the payload is intact");
        record = refused.into_inner();
        assert_eq!(sender.spare().unwrap_err(), Idle::Busy);
    }
    // The instant passed; the same payload goes through unchanged.
    sender.send(record).expect("the metadata is free again");
    assert_eq!(receiver.recv().expect("the payload").as_slice(), b"held");
}

#[test]
fn a_contended_return_keeps_its_block_instead_of_waiting() {
    let (mut sender, mut receiver) = handoff::<Vec<u8>>(2);
    sender.send(block()).unwrap();
    let taken = receiver.recv().expect("the payload");
    let held = sender.hold();
    let refused = receiver.give(taken).expect_err("the metadata was held");
    assert_eq!(refused.why, Refused::Busy);
    assert_eq!(refused.value.capacity(), 64, "the block is intact");
    assert_eq!(receiver.recv().unwrap_err(), Idle::Busy);
    drop(held);
    receiver.give(refused.into_inner()).expect("room again");
    assert_eq!(receiver.spares(), 1);
}

#[test]
fn a_send_after_the_consumer_is_gone_is_refused_with_its_payload() {
    let (mut sender, receiver) = handoff(2);
    sender.send(String::from("early")).unwrap();
    assert!(sender.taking());
    drop(receiver);

    assert!(!sender.taking(), "the consumer's departure is visible");
    let refused = sender
        .send(String::from("late"))
        .expect_err("the consumer is gone");
    assert_eq!(refused.why, Refused::Closed);
    assert_eq!(refused.value, "late", "the payload came back");
    // Permanent: a retry loop stops here rather than spinning.
    assert_eq!(
        sender.send(refused.into_inner()).unwrap_err().why,
        Refused::Closed
    );
    assert_eq!(sender.spare().unwrap_err(), Idle::Closed);
}

#[test]
fn accepted_payloads_do_not_outlive_the_consumer_that_never_took_them() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let (mut sender, receiver) = handoff(2);
    sender.send(Counted(Arc::clone(&dropped))).unwrap();
    assert_eq!(dropped.load(SeqCst), 0, "acceptance is not a drop");
    // Acceptance is not delivery: what the consumer never took goes with it. The queue's promise
    // is that it never drops a payload *instead of* telling the sender, not that it survives a
    // consumer that disappears.
    drop(receiver);
    drop(sender);
    assert_eq!(dropped.load(SeqCst), 1);
}

#[test]
fn a_consumer_drains_what_was_sent_before_it_learns_the_producer_is_gone() {
    let (mut sender, mut receiver) = handoff(4);
    sender.send(1u8).unwrap();
    sender.send(2u8).unwrap();
    assert!(receiver.sending());
    drop(sender);

    // Empty and closed are different answers, and this is why: the consumer has to finish.
    assert!(!receiver.sending());
    assert_eq!(receiver.recv().unwrap(), 1);
    assert_eq!(receiver.recv().unwrap(), 2);
    assert_eq!(receiver.recv().unwrap_err(), Idle::Closed);
    assert_eq!(receiver.recv().unwrap_err(), Idle::Closed, "permanent");
    // Recycling storage for a producer that is gone is refused, not silently swallowed.
    assert_eq!(receiver.give(3u8).unwrap_err().why, Refused::Closed);
}

#[test]
fn every_accepted_payload_arrives_exactly_once_and_in_order() {
    let (mut sender, mut receiver) = handoff(8);
    let records = 50_000u64;
    thread::scope(|threads| {
        let consuming = threads.spawn(move || {
            let mut want = 0u64;
            let mut taken = 0u64;
            while want < records {
                let record = match receiver.recv() {
                    Ok(record) => record,
                    // Nothing there, or one contended instant: both are "come back", and
                    // neither may be read as the end of the stream.
                    Err(Idle::Empty | Idle::Busy) => {
                        thread::yield_now();
                        continue;
                    }
                    Err(Idle::Closed) => panic!("the producer is still sending"),
                };
                assert_eq!(record, want, "payloads arrive in the order they were sent");
                want += 1;
                taken += 1;
            }
            taken
        });
        let mut next = 0u64;
        while next < records {
            match sender.send(next) {
                Ok(()) => next += 1,
                // A refusal is the whole backpressure story: the payload came back and is sent
                // again here rather than being lost. Neither reason is an error, and the caller
                // keeps owning the value until it is accepted.
                Err(Unsent {
                    why: Refused::Full | Refused::Busy,
                    value,
                }) => {
                    assert_eq!(value, next);
                    thread::yield_now();
                }
                Err(Unsent {
                    why: Refused::Closed,
                    ..
                }) => panic!("the consumer is still receiving"),
            }
        }
        assert_eq!(consuming.join().unwrap(), records);
    });
}
