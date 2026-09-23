// `concurrent!`, `Exclusive` and `Handoff` through `crate::`, so the same falsifiers run against
// whichever backend is selected.

use crate::Step;
use crate::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed, Ordering::SeqCst};
use crate::sync::handoff::{Handoff, Idle, Refused, Unsent};
use crate::sync::{Exclusive, Locked};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn a_held_value_is_busy_at_once_and_free_after_its_call() {
    let value = Exclusive::new(7u32);
    crate::concurrent! {
        || {
            value
                .with(|v| {
                    *v += 1;
                    assert_eq!(value.with(|v| *v), Err(Locked::Busy), "one attempt, no wait");
                })
                .expect("nobody holds it");
            assert_eq!(value.with(|v| *v), Ok(8), "released");
            Ok::<_, ()>(Step::Done)
        },
    }
    .expect("the arm succeeds");
    assert_eq!(value.into_inner(), 8);
}

#[test]
fn with_returns_no_uninit_bool_and_u64_values() {
    let value = Exclusive::new(7u64);
    assert_eq!(value.with(|n| *n == 7), Ok(true));
    assert_eq!(value.with(|n| *n + 1), Ok(8));
}

#[test]
fn a_step_that_catches_its_own_panic_leaves_the_value_abandoned_for_its_sibling() {
    let mut value = Exclusive::new(0u32);
    let caught = AtomicBool::new(false);
    let mut seen = None;
    crate::concurrent! {
        || {
            let unwound = catch_unwind(AssertUnwindSafe(|| {
                value.with::<()>(|v| {
                    *v = 1;
                    panic!("torn");
                })
            }));
            caught.store(unwound.is_err(), SeqCst);
            Ok::<_, ()>(Step::Done)
        },
        || {
            if !caught.load(SeqCst) {
                return Ok(Step::Idle);
            }
            seen = value.with(|v| *v).err();
            Ok(Step::Done)
        },
    }
    .expect("no arm fails");
    assert_eq!(seen, Some(Locked::Abandoned));
    assert_eq!(*value.get_mut(), 1, "the owner still reaches the value");
}

#[test]
fn a_panicking_step_stops_an_idle_sibling_before_the_scoped_join() {
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        crate::concurrent! {
            || Ok::<_, ()>(Step::Idle),
            || panic!("step"),
        }
    }));
    assert_eq!(
        unwound
            .expect_err("the arm panic propagates")
            .downcast_ref::<&str>(),
        Some(&"step")
    );
}

fn count_to(n: u32, done: &mut u32) -> Result<Step, u32> {
    *done += 1;
    Ok(if *done == n {
        Step::Done
    } else {
        Step::Progress
    })
}

#[test]
fn every_arm_runs_until_done_and_is_not_stepped_after() {
    let (mut a, mut b, mut c) = (0, 0, 0);
    crate::concurrent! {
        || count_to(3, &mut a),
        || count_to(5, &mut b),
        || count_to(1, &mut c),
    }
    .expect("no arm fails");
    assert_eq!((a, b, c), (3, 5, 1));
}

#[test]
fn every_block_cycles_in_fifo_order_and_a_refusal_returns_the_payload() {
    let mut made = 0u32;
    let mut handoff = Handoff::<u32, 4>::new(|| {
        made += 1;
        made * 10
    });
    for round in 0..3u32 {
        let (mut tx, mut rx) = handoff.split();
        let mut sent = Vec::new();
        while let Ok(block) = tx.spare() {
            let block = block + round;
            tx.send(block).expect("room for every spare");
            sent.push(block);
        }
        assert_eq!(sent.len(), 4, "all D blocks were spare");
        assert_eq!(tx.spare(), Err(Idle::Empty));
        assert_eq!(
            tx.send(99),
            Err(Unsent {
                why: Refused::Full,
                value: 99
            })
        );
        let got: Vec<u32> = core::iter::from_fn(|| rx.recv().ok()).collect();
        assert_eq!(got, sent, "FIFO");
        assert_eq!(rx.recv(), Err(Idle::Empty));
        for block in got {
            rx.give(block - round).expect("room for every block");
        }
        assert_eq!(
            rx.give(98),
            Err(Unsent {
                why: Refused::Full,
                value: 98
            })
        );
    }
    assert_eq!(made, 4, "no block was built after construction");
}

#[test]
fn a_dropped_endpoint_is_closed_and_a_new_split_reopens() {
    let mut handoff = Handoff::<u32, 2>::new(|| 1);
    {
        let (mut tx, rx) = handoff.split();
        let block = tx.spare().expect("stocked");
        drop(rx);
        assert_eq!(
            tx.send(block),
            Err(Unsent {
                why: Refused::Closed,
                value: 1
            })
        );
        assert_eq!(tx.spare(), Ok(1));
        assert_eq!(tx.spare(), Err(Idle::Closed));
    }
    let (mut tx, mut rx) = handoff.split();
    assert!(rx.sending());
    tx.send(3).expect("reopened");
    drop(tx);
    assert!(!rx.sending());
    assert_eq!(rx.recv(), Ok(3), "what was sent before the drop is still taken");
    assert_eq!(rx.recv(), Err(Idle::Closed));
}

const DEPTH: usize = 4;
const ITEMS: u32 = 1000;

#[test]
fn a_concurrent_handoff_preserves_fifo_values_and_all_four_spares() {
    let mut handoff = Handoff::<u32, DEPTH>::new(|| 0);
    let (tx, mut rx) = handoff.split();
    let mut tx = Some(tx);
    let mut next = 0;
    let mut pending = None;
    let mut spares = Vec::new();
    let mut received = 0;
    let mut back = None;

    crate::concurrent! {
        || {
            if next < ITEMS {
                let result = {
                    let sender = tx.as_mut().expect("not stepped after Done");
                    let value = match pending.take() {
                        Some(value) => value,
                        None => match sender.spare() {
                            Ok(_) => next,
                            Err(Idle::Empty | Idle::Busy) => return Ok(Step::Idle),
                            Err(Idle::Closed) => return Err("receiver closed early"),
                        },
                    };
                    sender.send(value)
                };
                return match result {
                    Ok(()) => {
                        next += 1;
                        Ok(Step::Progress)
                    }
                    Err(Unsent { why: Refused::Busy | Refused::Full, value }) => {
                        pending = Some(value);
                        Ok(Step::Idle)
                    }
                    Err(Unsent { why: Refused::Closed, .. }) => Err("receiver closed early"),
                };
            }

            let value = match tx.as_mut().expect("sender stays live until spares return").spare() {
                Ok(value) => value,
                Err(Idle::Empty | Idle::Busy) => return Ok(Step::Idle),
                Err(Idle::Closed) => return Err("receiver closed early"),
            };
            spares.push(value);
            if spares.len() == DEPTH {
                tx = None;
                Ok(Step::Done)
            } else {
                Ok(Step::Progress)
            }
        },
        || {
            if let Some(value) = back.take() {
                match rx.give(value) {
                    Ok(()) => {}
                    Err(Unsent { why: Refused::Busy | Refused::Full, value }) => {
                        back = Some(value);
                        return Ok(Step::Idle);
                    }
                    Err(Unsent { why: Refused::Closed, .. }) => return Err("sender closed early"),
                }
            }
            match rx.recv() {
                Ok(value) => {
                    assert_eq!(value, received, "FIFO");
                    received += 1;
                    back = Some(value);
                    Ok(Step::Progress)
                }
                Err(Idle::Empty | Idle::Busy) => Ok(Step::Idle),
                Err(Idle::Closed) => {
                    assert_eq!(received, ITEMS);
                    Ok(Step::Done)
                }
            }
        },
    }
    .expect("the producer and consumer complete");

    assert_eq!((next, received), (ITEMS, ITEMS));
    assert!(pending.is_none() && back.is_none());
    assert_eq!(
        spares.len(),
        DEPTH,
        "the closed, drained queue leaves all D slots as spares"
    );
}

#[test]
fn atomic_defaults_are_false_or_zero() {
    assert!(!AtomicBool::default().load(Relaxed));
    assert_eq!(AtomicU32::default().load(Relaxed), 0);
    assert_eq!(AtomicU64::default().load(Relaxed), 0);
}
