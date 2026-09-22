// Bounded interleaving exploration with `loom`, compiled only under `--cfg loom`:
//
//     RUSTFLAGS="--cfg loom" cargo test -p trame --lib sync::tests::interleaving
//
// What loom actually explores here: every permitted interleaving of the *loom* primitives these
// models use — `Mutex` acquisition order, `thread::park`/`unpark` including a wake-up that
// arrives before the sleep, and the `SeqCst` flag inside `Cancel` — for the small number of
// operations each model performs. That is the part of these primitives where a lost wake-up or a
// missed hand-off would live, and a model that parks forever is reported as a deadlock rather
// than hanging a test run.
//
// The nonblocking paths are modelled here too, because `try_lock` failing is an interleaving
// like any other: loom's mutex refuses a `try_lock` exactly when another thread holds the lock,
// so a model with a peer in its critical section explores both the accepted and the refused
// answer, and the assertions below are about what the refused one costs (nothing).
//
// What it does not explore: anything above the primitives, payload sizes, thread counts beyond
// the two or three below, the real operating system's scheduler, or the capacity refusals, which
// are panics — unwinding out of a loom model leaves its execution state behind, so the overflow
// cases are checked with real threads in `tests/turn.rs` instead. Loom checks a model of the
// protocol as written, not the compiled binary. The stress tests next door run the real thing;
// neither is a substitute for the other, and neither is a proof of the whole family.

use loom::thread;

use crate::cpu::sync::handoff::{Idle, Refused, handoff};
use crate::cpu::sync::publish::{Pressure, Version, published};
use crate::cpu::sync::rt::Arc;
use crate::cpu::sync::turn::{Exclusive, NORMAL, Priority};
use crate::cpu::sync::{Cancel, Cancelled};

#[test]
fn two_callers_take_the_turn_one_at_a_time() {
    loom::model(|| {
        let value = Arc::new(Exclusive::new(0u32));
        let other = Arc::clone(&value);
        let worker = thread::spawn(move || {
            *other.lock() += 1;
        });
        *value.lock() += 1;
        worker.join().unwrap();
        assert_eq!(*value.lock(), 2, "an increment was lost");
    });
}

#[test]
fn a_waiting_caller_is_always_woken_by_the_release() {
    loom::model(|| {
        let value = Arc::new(Exclusive::new(0u32));
        let other = Arc::clone(&value);
        // The spawned caller may find the turn free or may have to park for it; both paths, and
        // the release that races them, are explored.
        let worker = thread::spawn(move || {
            *other.take(Priority(2)) += 1;
        });
        {
            let mut held = value.lock();
            *held += 1;
        }
        worker.join().unwrap();
        assert_eq!(*value.lock(), 2);
        assert_eq!(value.waiters(), 0);
    });
}

#[test]
fn a_cancel_racing_a_hand_off_leaves_one_consistent_outcome() {
    loom::model(|| {
        let value = Arc::new(Exclusive::new(0u32));
        let cancel = Arc::new(Cancel::new());
        let (mine, token) = (Arc::clone(&value), Arc::clone(&cancel));
        let waiter = thread::spawn(move || {
            mine.take_until(NORMAL, &token).map(|mut turn| {
                *turn += 1;
            })
        });
        {
            let mut held = value.lock();
            *held += 10;
        }
        cancel.raise();
        let outcome = waiter.join().unwrap();
        // Either the turn was taken or the wait was cancelled: never both, and never neither.
        let took = outcome != Err(Cancelled);
        assert_eq!(*value.lock(), if took { 11 } else { 10 });
        assert_eq!(value.waiters(), 0, "the waiter deregistered either way");
        assert!(
            value.try_lock().is_some(),
            "the turn was left held by nobody"
        );
    });
}

#[test]
fn a_try_lock_that_loses_the_bookkeeping_race_leaves_the_turn_acquirable() {
    loom::model(|| {
        let value = Arc::new(Exclusive::new(0u32));
        let other = Arc::clone(&value);
        // Every interleaving of the two: the turn free or held, and the metadata lock free or
        // held by the caller that is taking it. `try_lock` returns either way and never waits.
        let worker = thread::spawn(move || {
            if let Some(mut turn) = other.try_lock() {
                *turn += 1;
            }
        });
        *value.lock() += 10;
        worker.join().unwrap();
        let seen = *value.lock();
        assert!(seen == 10 || seen == 11, "a turn was taken twice: {seen}");
        assert_eq!(value.waiters(), 0);
        assert!(
            value.try_lock().is_some(),
            "a refused try_lock left the turn claimed by nobody"
        );
    });
}

#[test]
fn a_cancel_raised_before_a_wait_never_takes_a_turn_it_must_release() {
    loom::model(|| {
        let value = Arc::new(Exclusive::new(0u32));
        let cancel = Arc::new(Cancel::new());
        let (mine, token) = (Arc::clone(&value), Arc::clone(&cancel));
        cancel.raise();
        let waiter = thread::spawn(move || mine.take_until(NORMAL, &token).map(|turn| *turn));
        // The turn is free the whole time, and the raise is ordered before the call: the answer
        // is `Cancelled` in every interleaving, with nothing registered and nothing held.
        assert_eq!(waiter.join().unwrap(), Err(Cancelled));
        assert_eq!(value.waiters(), 0);
        assert!(value.try_lock().is_some());
    });
}

#[test]
fn a_publication_refused_for_contention_keeps_its_draft() {
    loom::model(|| {
        let (mut writer, mut reader) = published(2, || 0u64);
        let reading = thread::spawn(move || reader.latest().map(|seen| *seen));
        *writer.draft() = 7;
        match writer.publish() {
            Ok(version) => assert_eq!(version.count(), 1),
            // The reader held the metadata at that instant. Nothing moved, so the draft is
            // still the writer's and is still what it wrote.
            Err(Pressure::Busy) => {
                assert_eq!(*writer.draft(), 7);
                assert_eq!(writer.version(), Version::NONE);
            }
            Err(Pressure::Full) => panic!("two buffers and no reader holding one"),
        }
        let seen = reading.join().unwrap();
        assert!(seen.is_none() || seen == Some(7));
    });
}

#[test]
fn a_handoff_refused_for_contention_gives_the_payload_back() {
    loom::model(|| {
        let (mut sender, mut receiver) = handoff(1);
        let taking = thread::spawn(move || receiver.recv());
        let sent = match sender.send(1u8) {
            Ok(()) => true,
            // Busy or Closed: either way the payload came back and nothing was queued.
            Err(back) => {
                assert_eq!(back.value, 1);
                assert!(matches!(back.why, Refused::Busy | Refused::Closed));
                false
            }
        };
        let taken = taking.join().unwrap();
        match taken {
            Ok(value) => assert!(sent && value == 1, "a payload appeared from nowhere"),
            // `Empty` and `Busy` say the consumer looked too early or lost the race for the
            // metadata; `Closed` cannot occur while this sender is alive.
            Err(why) => assert!(matches!(why, Idle::Empty | Idle::Busy), "{why:?}"),
        }
        let left = sender.pending();
        assert_eq!(left, usize::from(sent) - usize::from(taken.is_ok()));
    });
}

#[test]
fn a_reader_waiting_for_a_version_is_never_left_parked() {
    loom::model(|| {
        let (mut writer, mut reader) = published(2, || 0u64);
        let cancel = Arc::new(Cancel::new());
        let token = Arc::clone(&cancel);
        let reading = thread::spawn(move || reader.after(Version::NONE, &token).map(|seen| *seen));
        *writer.draft() = 7;
        writer
            .publish()
            .expect("the first publication has a buffer");
        // A lost wake-up would leave this model parked, which loom reports as a deadlock.
        assert_eq!(reading.join().unwrap().map_err(|_| ()), Ok(7));
    });
}

#[test]
fn a_reader_waiting_for_a_version_is_never_left_parked_by_a_cancel() {
    loom::model(|| {
        let (_writer, mut reader) = published(2, || 0u64);
        let cancel = Arc::new(Cancel::new());
        let token = Arc::clone(&cancel);
        let reading = thread::spawn(move || reader.after(Version::NONE, &token).map(|seen| *seen));
        cancel.raise();
        assert!(reading.join().unwrap().is_err());
    });
}

#[test]
fn a_published_version_is_whole_whenever_it_is_taken() {
    loom::model(|| {
        let (mut writer, mut reader) = published(3, || [0u64; 2]);
        let reading = thread::spawn(move || {
            let mut last = Version::NONE;
            for _ in 0..2 {
                if let Some(seen) = reader.latest() {
                    assert_eq!(seen[0], seen[1], "a half-written version was taken");
                    assert!(seen.version() >= last, "versions went backwards");
                    last = seen.version();
                }
            }
        });
        for count in 1..=2u64 {
            let draft = writer.draft();
            draft[0] = count;
            draft[1] = count;
            // The reader may hold the metadata, so publishing retries. Every attempt keeps the
            // draft, so the version that lands is whole either way.
            while let Err(pressure) = writer.publish() {
                assert_eq!(pressure, Pressure::Busy, "three buffers never fill up");
                thread::yield_now();
            }
        }
        reading.join().unwrap();
    });
}

#[test]
fn a_handed_payload_is_never_lost_or_duplicated() {
    loom::model(|| {
        let (mut sender, mut receiver) = handoff(2);
        let taking = thread::spawn(move || [receiver.recv(), receiver.recv()]);
        let mut sent = Vec::new();
        'sending: for value in 1..=2u8 {
            // Retried rather than expected: a contended instant is not a full queue, and the
            // payload comes back to be sent again. A consumer that has already finished and
            // dropped its handle ends the sending instead, which is the other thing a refusal
            // has to be able to say.
            let mut carried = value;
            loop {
                match sender.send(carried) {
                    Ok(()) => {
                        sent.push(value);
                        continue 'sending;
                    }
                    Err(back) if back.why == Refused::Busy => {
                        carried = back.into_inner();
                        thread::yield_now();
                    }
                    Err(back) => {
                        assert_eq!(back.why, Refused::Closed);
                        assert_eq!(back.value, value, "the payload came back");
                        break 'sending;
                    }
                }
            }
        }
        let taken: Vec<u8> = taking
            .join()
            .unwrap()
            .into_iter()
            .filter_map(Result::ok)
            .collect();
        // Whatever the consumer managed to take, it took a prefix of what was accepted, once
        // each, and the rest is still queued.
        assert!(
            taken.as_slice() == sent.get(..taken.len()).unwrap(),
            "payloads were lost, duplicated or reordered: {taken:?} of {sent:?}"
        );
        assert_eq!(sender.pending(), sent.len() - taken.len());
    });
}
