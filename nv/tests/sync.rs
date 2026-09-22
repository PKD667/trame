//! Family A on the host model, exercised by real contenders.
//!
//! The device family is this same code over device memory, so a test here is a test of the
//! algorithm and the *ordering* it promises: exclusion, FIFO among equals, cancellation, a
//! declared wait that runs out, and a bounded table that refuses rather than grows. What it
//! cannot certify is the memory model — two warps of one launch observing each other's stores —
//! and that is what the device run is for.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::nv::sync::turn::Refused;
use crate::nv::sync::{self, Cancel, Exclusive, NORMAL, WAITERS};

/// A turn and the value it guards, built the way a launch builds one: the words are prepared
/// first, then the addresses are handed over. The value is leaked so its address cannot move
/// under a contender, which is the same obligation a launch has.
fn turn() -> (Exclusive<u64>, Vec<u32>, *mut u64) {
    let mut words = vec![0u32; sync::WORDS];
    sync::init_header(&mut words);
    let value = Box::into_raw(Box::new(0u64));
    let exclusive = unsafe { Exclusive::new(words.as_mut_ptr(), value) };
    (exclusive, words, value)
}

/// Poll a condition that another thread is about to make true, with a bound so a failure is a
/// failure rather than a hang.
fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::yield_now();
    }
}

#[test]
fn the_turn_keeps_two_warps_out_of_the_value_at_once() {
    const THREADS: u64 = 4;
    const ROUNDS: u64 = 5_000;
    let (exclusive, _words, value) = turn();
    let exclusive = &exclusive;

    thread::scope(|scope| {
        for _ in 0..THREADS {
            scope.spawn(move || {
                for _ in 0..ROUNDS {
                    // The guard is the only way to the value, so this is an ordinary
                    // read-modify-write and any overlap between two holders loses an update that
                    // the total below would be short by.
                    let mut held = exclusive.take(NORMAL, 0).expect("an uncontended turn");
                    *held += 1;
                }
            });
        }
    });

    let total = unsafe { *value };
    assert_eq!(
        total,
        THREADS * ROUNDS,
        "every increment survived, so no two holders overlapped"
    );
}

#[test]
fn a_waiting_caller_is_served_in_registration_order() {
    // `WAITERS` seats, one of them the holder, so three contenders can be ordered exactly.
    const CONTENDERS: usize = WAITERS - 1;
    let (exclusive, _words, _value) = turn();
    let exclusive = &exclusive;

    let (order_tx, order_rx) = mpsc::channel::<usize>();
    // A receiver is not cloneable, so each contender is released on its own channel rather than
    // all of them watching one.
    let mut release = Vec::new();

    thread::scope(|scope| {
        // The holder takes the turn first, so everything below registers behind it.
        let held = exclusive.take(NORMAL, 0).expect("the first turn");

        for index in 0..CONTENDERS {
            let (go_tx, go_rx) = mpsc::channel::<()>();
            release.push(go_tx);
            let order_tx = order_tx.clone();
            scope.spawn(move || {
                go_rx.recv().expect("told to go");
                let turn = exclusive.take(NORMAL, 0).expect("the turn comes round");
                order_tx.send(index).expect("its turn");
                drop(turn);
            });
        }

        // Regression order is made exact rather than hoped for: each contender is released to
        // register only once the previous one is *in the table*. The holder occupies a seat too,
        // which is why the count starts at one.
        for (index, go_tx) in release.iter().enumerate() {
            go_tx.send(()).expect("release the next contender");
            until("a contender to register", || {
                exclusive.waiters() == index + 1
            });
        }
        drop(held);

        let observed: Vec<usize> = order_rx.iter().take(CONTENDERS).collect();
        assert_eq!(
            observed,
            (0..CONTENDERS).collect::<Vec<_>>(),
            "earliest registered, served first"
        );
    });
}

#[test]
fn cancellation_ends_a_wait_without_losing_the_turn() {
    let (exclusive, _words, _value) = turn();
    let exclusive = &exclusive;
    let cancel = Arc::new(Cancel::default());

    thread::scope(|scope| {
        let held = exclusive.take(NORMAL, 0).expect("the first turn");

        let waiter = {
            let cancel = Arc::clone(&cancel);
            scope.spawn(move || exclusive.take_until(NORMAL, 0, &cancel))
        };
        until("the waiter to register", || exclusive.waiters() == 1);

        cancel.raise();
        let outcome = waiter.join().expect("the waiter did not panic");
        assert_eq!(outcome.err(), Some(Refused::Cancelled));
        assert_eq!(
            exclusive.waiters(),
            0,
            "a cancelled wait leaves nothing registered"
        );

        drop(held);
        // The turn is still there: a cancelled wait consumed nothing.
        let next = exclusive
            .take(NORMAL, 0)
            .expect("the turn outlived the cancellation");
        drop(next);
    });
}

#[test]
fn a_wait_that_runs_out_of_attempts_reports_exhaustion_and_keeps_the_turn() {
    let (exclusive, _words, _value) = turn();
    let exclusive = &exclusive;

    let held = exclusive.take(NORMAL, 0).expect("the first turn");
    // One attempt against a held turn: a poll, reported as exhaustion rather than as capacity.
    assert_eq!(
        exclusive.take(NORMAL, 1).err(),
        Some(Refused::Exhausted { attempts: 1 })
    );
    assert_eq!(
        exclusive.waiters(),
        0,
        "an exhausted wait leaves nothing registered"
    );

    drop(held);
    let next = exclusive
        .take(NORMAL, 1)
        .expect("the turn was not consumed by the attempt");
    drop(next);
}

#[test]
fn more_registrations_than_seats_are_refused_as_capacity() {
    let (exclusive, _words, _value) = turn();
    let exclusive = &exclusive;
    let mut release = Vec::new();

    thread::scope(|scope| {
        let held = exclusive.take(NORMAL, 0).expect("the first turn");

        for _ in 0..WAITERS - 1 {
            let (go_tx, go_rx) = mpsc::channel::<()>();
            release.push(go_tx);
            scope.spawn(move || {
                go_rx.recv().expect("told to go");
                // These hold the turn until the scope ends, so they stay registered.
                let turn = exclusive.take(NORMAL, 0).expect("a seat is free");
                let _ = turn;
                std::thread::sleep(Duration::from_millis(200));
            });
        }
        for go_tx in &release {
            go_tx.send(()).expect("release a contender");
        }
        until("every seat to be taken", || {
            exclusive.waiters() == WAITERS - 1
        });

        // Every seat is taken, so this is capacity and not contention, and the two are told
        // apart rather than both reported as a busy turn.
        assert_eq!(exclusive.take(NORMAL, 0).err(), Some(Refused::Full));
        drop(held);
    });
}

#[test]
fn a_cancelled_caller_that_was_granted_the_turn_still_passes_it_on() {
    // The race this exists for: a cancellation that lands at the moment of the grant. The caller
    // owns the turn and never uses it, so a withdrawal that only cleared its seat would take the
    // turn out of circulation and every later caller would wait for it forever. A bounded wait is
    // used so that losing it is reported rather than hung on.
    const ROUNDS: usize = 200;
    let (exclusive, _words, _value) = turn();
    let exclusive = &exclusive;

    for _ in 0..ROUNDS {
        let cancel = Arc::new(Cancel::default());
        thread::scope(|scope| {
            let held = exclusive
                .take(NORMAL, 0)
                .expect("the turn comes back round");

            let doomed = {
                let cancel = Arc::clone(&cancel);
                scope.spawn(move || exclusive.take_until(NORMAL, 1 << 22, &cancel))
            };
            let survivor = scope.spawn(move || exclusive.take(NORMAL, 1 << 22));

            until("both contenders to register", || exclusive.waiters() == 2);
            cancel.raise();
            drop(held);

            let _ = doomed.join().expect("the cancelled caller did not panic");
            let outcome = survivor.join().expect("the survivor did not panic");
            assert!(
                outcome.is_ok(),
                "the turn was lost when a grant raced a cancellation: {:?}",
                outcome.err()
            );
        });
    }
}

#[test]
fn a_poll_that_finds_the_turn_taken_leaves_nothing_registered() {
    let (exclusive, _words, _value) = turn();
    let exclusive = &exclusive;

    let held = exclusive.take(NORMAL, 0).expect("the first turn");
    assert!(
        exclusive.try_lock().is_none(),
        "a poll against a held turn acquires nothing"
    );
    assert_eq!(exclusive.waiters(), 0, "and registers nothing");
    drop(held);
    assert!(
        exclusive.try_lock().is_some(),
        "a poll against a free turn acquires it"
    );
}

#[test]
fn lock_is_the_unrestricted_form_and_spells_the_same_as_the_host_family() {
    // A portable call site writes `.lock()`, so the device family has to answer to that name and
    // not only to the forms that state a bound.
    let (exclusive, _words, _value) = turn();
    let held = exclusive.lock();
    assert_eq!(*held, 0);
    drop(held);
    assert!(
        exclusive.try_lock().is_some(),
        "and releasing it leaves the turn free"
    );
}

#[test]
fn the_value_is_reached_through_the_guard_and_not_around_it() {
    // The guard is the only way to the value, and this is the shape a caller relies on: the
    // family lends the state for one call rather than handing out an owner.
    let (exclusive, _words, _value) = turn();
    {
        let mut held = exclusive.take(NORMAL, 0).expect("the first turn");
        *held += 41;
        *held += 1;
    }
    let held = exclusive
        .try_lock()
        .expect("free again after the guard dropped");
    assert_eq!(*held, 42);
}
