// Family A: who gets the turn next, and what happens to a caller that never gets it.
//
// Every test that depends on registration order establishes it with `registered`, which waits
// for the waiter count rather than sleeping: a waiter is registered when `waiters` counts it,
// and the hand-off rule is stated over registered waiters.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

use crate::cpu::sync::turn::{Exclusive, NORMAL, Priority};
use crate::cpu::sync::{Cancel, Cancelled};

/// Wait until `count` callers are registered as waiting, or fail rather than hang.
fn registered<T>(on: &Exclusive<T>, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while on.waiters() < count {
        assert!(Instant::now() < deadline, "waiters never reached {count}");
        thread::yield_now();
    }
}

/// The text a panic carried, whichever way it was raised.
fn message(payload: Box<dyn Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

#[test]
fn an_uncontended_turn_is_taken_and_given_back() {
    let value = Exclusive::new(1u32);
    {
        let mut turn = value.lock();
        *turn += 1;
    }
    assert_eq!(value.waiters(), 0);
    assert_eq!(*value.lock(), 2);
    assert_eq!(value.into_inner(), 2);
}

#[test]
fn a_held_turn_excludes_everyone_else() {
    let value = Exclusive::new(0u64);
    thread::scope(|threads| {
        for _ in 0..4 {
            threads.spawn(|| {
                for _ in 0..1000 {
                    *value.lock() += 1;
                }
            });
        }
    });
    assert_eq!(value.into_inner(), 4000);
}

#[test]
fn equal_priorities_take_the_turn_in_registration_order() {
    let value = &Exclusive::with_waiters(0u32, 3);
    let order = &Mutex::new(Vec::new());
    thread::scope(|threads| {
        let held = value.lock();
        for who in 1..=3u32 {
            threads.spawn(move || {
                let _turn = value.lock();
                order.lock().unwrap().push(who);
            });
            // Registration order is what FIFO is about, so establish it before the next spawn.
            registered(value, who as usize);
        }
        drop(held);
    });
    assert_eq!(order.lock().unwrap().as_slice(), [1, 2, 3]);
}

#[test]
fn a_later_higher_priority_waiter_takes_the_turn_first() {
    let value = &Exclusive::with_waiters(0u32, 3);
    let order = &Mutex::new(Vec::new());
    thread::scope(|threads| {
        let held = value.lock();

        // Registered first, and lowest: it must lose to the ones registered after it.
        threads.spawn(move || {
            let _turn = value.take(Priority(1));
            order.lock().unwrap().push("low");
        });
        registered(value, 1);

        threads.spawn(move || {
            let _turn = value.take(Priority(2));
            order.lock().unwrap().push("middle");
        });
        registered(value, 2);

        threads.spawn(move || {
            let _turn = value.take(Priority(9));
            order.lock().unwrap().push("high");
        });
        registered(value, 3);

        drop(held);
    });
    // A priority-ignoring implementation hands these out in registration order and fails here.
    assert_eq!(order.lock().unwrap().as_slice(), ["high", "middle", "low"]);
}

#[test]
fn a_granted_turn_cannot_be_barged_past() {
    let value = Exclusive::new(0u32);
    thread::scope(|threads| {
        let held = value.lock();
        let waiter = threads.spawn(|| {
            let mut turn = value.lock();
            *turn += 1;
            // Held long enough that an unfair primitive would have a window to barge.
            thread::sleep(Duration::from_millis(20));
        });
        registered(&value, 1);
        // Dropping hands the turn to the registered waiter; the resource stays held whether or
        // not that waiter has woken up yet, so nobody else can take it.
        drop(held);
        assert!(
            value.try_lock().is_none(),
            "try_lock took a turn that had been handed to a waiter"
        );
        waiter.join().unwrap();
    });
    assert_eq!(value.into_inner(), 1);
}

#[test]
fn try_lock_takes_a_free_turn_and_nothing_else() {
    let value = Exclusive::new(7u32);
    let held = value.try_lock().expect("a free turn");
    assert!(value.try_lock().is_none());
    assert_eq!(value.waiters(), 0, "try_lock never registers");
    drop(held);
    assert!(value.try_lock().is_some());
}

#[test]
fn a_cancelled_wait_leaves_the_resource_acquirable() {
    let value = Exclusive::new(0u32);
    let cancel = Cancel::new();
    thread::scope(|threads| {
        let held = value.lock();
        let waiter = threads.spawn(|| value.take_until(NORMAL, &cancel).map(|turn| *turn));
        registered(&value, 1);
        cancel.raise();
        assert_eq!(waiter.join().unwrap(), Err(Cancelled));
        assert_eq!(value.waiters(), 0, "a cancelled waiter deregisters");
        drop(held);
    });
    assert!(value.try_lock().is_some(), "the turn was never handed out");
}

#[test]
fn a_cancel_raised_before_the_call_ends_it_without_acquiring() {
    let value = Exclusive::new(0u32);
    let cancel = Cancel::new();
    cancel.raise();
    let held = value.lock();
    assert_eq!(
        value.take_until(NORMAL, &cancel).map(|turn| *turn),
        Err(Cancelled)
    );
    drop(held);
    // The turn is free now, and the answer does not change: a raise ordered before the call is
    // seen by its first check, so the caller never takes a resource it was told to stop for.
    // Only an acquisition that genuinely won the race against a concurrent raise survives it.
    assert_eq!(
        value.take_until(NORMAL, &cancel).map(|turn| *turn),
        Err(Cancelled)
    );
    assert_eq!(value.waiters(), 0, "nothing was registered");
    assert!(
        value.try_lock().is_some(),
        "the resource was left acquirable"
    );
}

#[test]
fn a_cancel_that_arrives_after_the_hand_off_does_not_lose_the_turn() {
    let value = Exclusive::new(0u32);
    let cancel = Cancel::new();
    thread::scope(|threads| {
        let held = value.lock();
        let waiter = threads.spawn(|| {
            value.take_until(NORMAL, &cancel).map(|mut turn| {
                *turn += 1;
            })
        });
        registered(&value, 1);
        // The grant is set inside this drop, before the cancel below. A waiter checks its grant
        // before it checks the token, so it takes the turn rather than reporting `Cancelled`.
        drop(held);
        cancel.raise();
        assert_eq!(waiter.join().unwrap(), Ok(()));
    });
    assert_eq!(value.into_inner(), 1, "the handed-off turn was used");
}

#[test]
fn a_cancelled_waiter_does_not_swallow_the_turn() {
    let value = Exclusive::with_waiters(0u32, 2);
    let cancel = Cancel::new();
    thread::scope(|threads| {
        let held = value.lock();
        let leaving = threads.spawn(|| value.take_until(NORMAL, &cancel).map(|turn| *turn));
        registered(&value, 1);
        let staying = threads.spawn(|| {
            let mut turn = value.lock();
            *turn += 5;
        });
        registered(&value, 2);
        cancel.raise();
        assert_eq!(leaving.join().unwrap(), Err(Cancelled));
        drop(held);
        staying.join().unwrap();
    });
    assert_eq!(value.into_inner(), 5);
}

#[test]
fn an_unwinding_holder_hands_the_turn_on_and_poisons_it() {
    let value = &Exclusive::new(0u32);
    thread::scope(|threads| {
        let (held, is_held) = channel();
        let holder = threads.spawn(move || {
            catch_unwind(AssertUnwindSafe(|| {
                let _turn = value.lock();
                held.send(()).unwrap();
                // Panic only once somebody is waiting, so the unwinding release has a waiter to
                // wake: a waiter left parked here would be the defect this test looks for.
                registered(value, 1);
                panic!("holder gives up");
            }))
        });
        is_held.recv().unwrap();
        let waiter = threads.spawn(move || {
            catch_unwind(AssertUnwindSafe(|| {
                let _turn = value.lock();
            }))
        });
        assert!(holder.join().unwrap().is_err());
        // Being handed a poisoned resource is fatal for the waiter too.
        assert!(waiter.join().unwrap().is_err());
    });
    assert!(value.poisoned());
    let fatal = catch_unwind(AssertUnwindSafe(|| {
        value.lock();
    }))
    .expect_err("locking a poisoned resource is fatal");
    assert!(message(fatal).contains("poisoned"));
    let fatal = catch_unwind(AssertUnwindSafe(|| {
        value.try_lock();
    }))
    .expect_err("try_lock on a poisoned resource is fatal");
    assert!(message(fatal).contains("poisoned"));
}

#[test]
fn an_exhausted_ticket_counter_is_refused_before_anything_is_registered() {
    // Tickets are identities and the tie-break that makes equal priorities FIFO. A counter that
    // wrapped would hand two live waiters one number, so the check comes first and the refused
    // caller leaves the table exactly as it found it.
    let value = Exclusive::with_waiters(0u32, 4);
    value.tickets_from(u64::MAX);
    thread::scope(|threads| {
        let held = value.lock();
        let refused = threads
            .spawn(|| catch_unwind(AssertUnwindSafe(|| value.lock())).map(|_| ()))
            .join()
            .unwrap()
            .unwrap_err();
        assert!(message(refused).contains("ticket counter is exhausted"));
        assert_eq!(value.waiters(), 0, "nothing was registered");
        drop(held);
    });
    assert!(!value.poisoned(), "a refused registration held nothing");
    assert!(value.try_lock().is_some(), "the turn was handed to nobody");
}

#[test]
fn more_waiters_than_the_table_holds_is_refused_explicitly() {
    let value = Exclusive::with_waiters(0u32, 1);
    thread::scope(|threads| {
        let held = value.lock();
        let first = threads.spawn(|| {
            let _turn = value.lock();
        });
        registered(&value, 1);
        let refused = threads
            .spawn(|| catch_unwind(AssertUnwindSafe(|| value.lock())).map(|_| ()))
            .join()
            .unwrap()
            .unwrap_err();
        assert!(message(refused).contains("more than 1 waiting callers"));
        drop(held);
        first.join().unwrap();
    });
    assert!(!value.poisoned(), "a refused registration held nothing");
}

#[test]
fn more_cancel_watchers_than_the_table_holds_is_refused_explicitly() {
    let value = Exclusive::with_waiters(0u32, 4);
    let cancel = Cancel::with_capacity(1);
    thread::scope(|threads| {
        let held = value.lock();
        let first = threads.spawn(|| value.take_until(NORMAL, &cancel).map(|turn| *turn));
        registered(&value, 1);
        let refused = threads
            .spawn(|| {
                catch_unwind(AssertUnwindSafe(|| {
                    value.take_until(NORMAL, &cancel).map(|_| ())
                }))
            })
            .join()
            .unwrap()
            .unwrap_err();
        assert!(message(refused).contains("more than 1 waiting callers"));
        cancel.raise();
        assert_eq!(first.join().unwrap(), Err(Cancelled));
        drop(held);
    });
}

#[test]
fn a_refused_cancel_registration_leaves_no_ticket_behind() {
    // The defect this is written against: a caller that takes a ticket and *then* registers with
    // the token has, when the token's table refuses it, left a registration in the queue whose
    // caller is gone. The release hands the turn to that ghost, and the resource is never
    // acquirable again. Registration therefore comes first, and this is what that means.
    let value = Exclusive::with_waiters(0u32, 4);
    let cancel = Cancel::with_capacity(1);
    let held = value.lock();
    thread::scope(|threads| {
        // One watcher, which fills the token's table and keeps it full.
        let waiting = threads.spawn(|| value.take_until(NORMAL, &cancel).map(|turn| *turn));
        registered(&value, 1);

        // A second caller is refused by the token. It survives the panic, as a supervised
        // thread or a `catch_unwind` boundary would.
        let refused = threads
            .spawn(|| {
                catch_unwind(AssertUnwindSafe(|| {
                    value.take_until(NORMAL, &cancel).map(|_| ())
                }))
            })
            .join()
            .unwrap()
            .unwrap_err();
        assert!(message(refused).contains("more than 1 waiting callers"));

        // Released first, so that a defect shows up as a failed assertion below rather than as
        // a test that hangs waiting for a thread it cannot wake.
        cancel.raise();
        assert_eq!(waiting.join().unwrap(), Err(Cancelled));
    });
    assert_eq!(
        value.waiters(),
        0,
        "the refused caller left a ticket behind on the resource"
    );

    // The whole point: the release has nobody to hand the turn to, so it stays acquirable.
    drop(held);
    let mut turn = value.try_lock().expect("the turn was handed to nobody");
    *turn += 3;
    drop(turn);
    assert_eq!(value.into_inner(), 3);
}

#[test]
fn try_lock_reports_contended_bookkeeping_rather_than_waiting_for_it() {
    // `try_lock` may not wait for the metadata lock either: the caller holding it can be
    // descheduled, and a `try` that blocks is a `try` in name only.
    let value = &Exclusive::new(0u32);
    thread::scope(|threads| {
        let (busy, is_busy) = channel();
        let (go, carry_on) = channel();
        threads.spawn(move || {
            let bookkeeping = value.hold();
            busy.send(()).unwrap();
            carry_on.recv().unwrap();
            drop(bookkeeping);
        });
        is_busy.recv().unwrap();
        // The turn itself is free; only the bookkeeping is held. The answer is still `None`, and
        // it arrives rather than blocking this thread until the holder lets go.
        assert!(value.try_lock().is_none());
        go.send(()).unwrap();
    });
    assert!(value.try_lock().is_some(), "nothing was left claimed");
}
