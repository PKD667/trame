// `concurrent!`, `Exclusive` and `Handoff` through `crate::`, so the same falsifiers run against
// whichever backend is selected.

use crate::Step;
use crate::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed, Ordering::SeqCst};
use crate::sync::handoff::{Idle, Receiver, Refused, Sender, Unsent, give, new, recv, send, sending, spare, split};
use crate::sync::{Exclusive, Locked, with};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Takes the value, and tries it again from inside.
#[crate::process]
struct Nest<'a>(&'a Exclusive<u32>);

impl Nest<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        with(self.0, |v| {
            *v += 1;
            assert_eq!(with(self.0, |v| *v), Err(Locked::Busy), "one attempt, no wait");
        })
        .expect("nobody holds it");
        assert_eq!(with(self.0, |v| *v), Ok(8), "released");
        Ok(Step::Done)
    }
}

#[test]
fn a_held_value_is_busy_at_once_and_free_after_its_call() {
    let value = Exclusive::new(7u32);
    crate::concurrent!(Nest(&value)).expect("the arm succeeds");
    assert_eq!(value.into_inner(), 8);
}

#[test]
fn with_returns_no_uninit_bool_and_u64_values() {
    let value = Exclusive::new(7u64);
    assert_eq!(with(&value, |n| *n == 7), Ok(true));
    assert_eq!(with(&value, |n| *n + 1), Ok(8));
}

/// Panics inside the value and catches it, so the value is abandoned and the step is not.
#[crate::process]
struct Tear<'a> {
    value: &'a Exclusive<u32>,
    caught: &'a AtomicBool,
}

impl Tear<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        let unwound = catch_unwind(AssertUnwindSafe(|| {
            with::<_, ()>(self.value, |v| {
                *v = 1;
                panic!("torn");
            })
        }));
        self.caught.store(unwound.is_err(), SeqCst);
        Ok(Step::Done)
    }
}

/// Reads the value once the tear is over.
#[crate::process]
struct Reader<'a> {
    value: &'a Exclusive<u32>,
    caught: &'a AtomicBool,
    seen: Option<Locked>,
}

impl Reader<'_> {
    fn step(&mut self) -> Result<Step, ()> {
        if !self.caught.load(SeqCst) {
            return Ok(Step::Idle);
        }
        self.seen = with(self.value, |v| *v).err();
        Ok(Step::Done)
    }
}

#[test]
fn a_step_that_catches_its_own_panic_leaves_the_value_abandoned_for_its_sibling() {
    let mut value = Exclusive::new(0u32);
    let caught = AtomicBool::new(false);
    let mut reader = Reader { value: &value, caught: &caught, seen: None };
    crate::concurrent!(Tear { value: &value, caught: &caught }, &mut reader).expect("no arm fails");
    assert_eq!(reader.seen, Some(Locked::Abandoned));
    assert_eq!(*value.get_mut(), 1, "the owner still reaches the value");
}

#[crate::process]
struct Quiet;

impl Quiet {
    fn step(&mut self) -> Result<Step, ()> {
        Ok(Step::Idle)
    }
}

#[crate::process]
struct Panics;

impl Panics {
    fn step(&mut self) -> Result<Step, ()> {
        panic!("step")
    }
}

#[test]
fn a_panicking_step_stops_an_idle_sibling_before_the_scoped_join() {
    let unwound = catch_unwind(AssertUnwindSafe(|| crate::concurrent!(Quiet, Panics)));
    assert_eq!(
        unwound
            .expect_err("the arm panic propagates")
            .downcast_ref::<&str>(),
        Some(&"step")
    );
}

/// Counts its own steps up to `n`, then is done.
#[crate::process]
struct CountTo<'a> {
    n: u32,
    done: &'a mut u32,
}

impl CountTo<'_> {
    fn step(&mut self) -> Result<Step, u32> {
        *self.done += 1;
        Ok(if *self.done == self.n { Step::Done } else { Step::Progress })
    }
}

#[test]
fn every_arm_runs_until_done_and_is_not_stepped_after() {
    let (mut a, mut b, mut c) = (0, 0, 0);
    crate::concurrent!(
        CountTo { n: 3, done: &mut a },
        CountTo { n: 5, done: &mut b },
        CountTo { n: 1, done: &mut c },
    )
    .expect("no arm fails");
    assert_eq!((a, b, c), (3, 5, 1));
}

#[test]
fn every_block_cycles_in_fifo_order_and_a_refusal_returns_the_payload() {
    let mut made = 0u32;
    let mut handoff = new::<u32, 4>(|| {
        made += 1;
        made * 10
    });
    for round in 0..3u32 {
        let (mut tx, mut rx) = split(&mut handoff);
        let mut sent = Vec::new();
        while let Ok(block) = spare(&mut tx) {
            let block = block + round;
            send(&mut tx, block).expect("room for every spare");
            sent.push(block);
        }
        assert_eq!(sent.len(), 4, "all D blocks were spare");
        assert_eq!(spare(&mut tx), Err(Idle::Empty));
        assert_eq!(
            send(&mut tx, 99),
            Err(Unsent {
                why: Refused::Full,
                value: 99
            })
        );
        let got: Vec<u32> = core::iter::from_fn(|| recv(&mut rx).ok()).collect();
        assert_eq!(got, sent, "FIFO");
        assert_eq!(recv(&mut rx), Err(Idle::Empty));
        for block in got {
            give(&mut rx, block - round).expect("room for every block");
        }
        assert_eq!(
            give(&mut rx, 98),
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
    let mut handoff = new::<u32, 2>(|| 1);
    {
        let (mut tx, rx) = split(&mut handoff);
        let block = spare(&mut tx).expect("stocked");
        drop(rx);
        assert_eq!(
            send(&mut tx, block),
            Err(Unsent {
                why: Refused::Closed,
                value: 1
            })
        );
        assert_eq!(spare(&mut tx), Ok(1));
        assert_eq!(spare(&mut tx), Err(Idle::Closed));
    }
    let (mut tx, mut rx) = split(&mut handoff);
    assert!(sending(&rx));
    send(&mut tx, 3).expect("reopened");
    drop(tx);
    assert!(!sending(&rx));
    assert_eq!(recv(&mut rx), Ok(3), "what was sent before the drop is still taken");
    assert_eq!(recv(&mut rx), Err(Idle::Closed));
}

const DEPTH: usize = 4;
const ITEMS: u32 = 1000;

/// Sends `ITEMS` values, then takes its `DEPTH` spares back and closes.
#[crate::process]
struct Producer<'a> {
    tx: Option<Sender<'a, u32, DEPTH>>,
    next: u32,
    pending: Option<u32>,
    spares: Vec<u32>,
}

impl Producer<'_> {
    fn step(&mut self) -> Result<Step, &'static str> {
        if self.next < ITEMS {
            let sender = self.tx.as_mut().expect("not stepped after Done");
            let value = match self.pending.take() {
                Some(value) => value,
                None => match spare(sender) {
                    Ok(_) => self.next,
                    Err(Idle::Empty | Idle::Busy) => return Ok(Step::Idle),
                    Err(Idle::Closed) => return Err("receiver closed early"),
                },
            };
            return match send(sender, value) {
                Ok(()) => {
                    self.next += 1;
                    Ok(Step::Progress)
                }
                Err(Unsent { why: Refused::Busy | Refused::Full, value }) => {
                    self.pending = Some(value);
                    Ok(Step::Idle)
                }
                Err(Unsent { why: Refused::Closed, .. }) => Err("receiver closed early"),
            };
        }
        let value = match spare(self.tx.as_mut().expect("sender stays live until spares return")) {
            Ok(value) => value,
            Err(Idle::Empty | Idle::Busy) => return Ok(Step::Idle),
            Err(Idle::Closed) => return Err("receiver closed early"),
        };
        self.spares.push(value);
        if self.spares.len() == DEPTH {
            self.tx = None;
            Ok(Step::Done)
        } else {
            Ok(Step::Progress)
        }
    }
}

/// Takes every value in order and gives each back.
#[crate::process]
struct Consumer<'a> {
    rx: Receiver<'a, u32, DEPTH>,
    received: u32,
    back: Option<u32>,
}

impl Consumer<'_> {
    fn step(&mut self) -> Result<Step, &'static str> {
        if let Some(value) = self.back.take() {
            match give(&mut self.rx, value) {
                Ok(()) => {}
                Err(Unsent { why: Refused::Busy | Refused::Full, value }) => {
                    self.back = Some(value);
                    return Ok(Step::Idle);
                }
                Err(Unsent { why: Refused::Closed, .. }) => return Err("sender closed early"),
            }
        }
        match recv(&mut self.rx) {
            Ok(value) => {
                assert_eq!(value, self.received, "FIFO");
                self.received += 1;
                self.back = Some(value);
                Ok(Step::Progress)
            }
            Err(Idle::Empty | Idle::Busy) => Ok(Step::Idle),
            Err(Idle::Closed) => {
                assert_eq!(self.received, ITEMS);
                Ok(Step::Done)
            }
        }
    }
}

#[test]
fn a_concurrent_handoff_preserves_fifo_values_and_all_four_spares() {
    let mut handoff = new::<u32, DEPTH>(|| 0);
    let (tx, rx) = split(&mut handoff);
    let mut producer = Producer { tx: Some(tx), next: 0, pending: None, spares: Vec::new() };
    let mut consumer = Consumer { rx, received: 0, back: None };
    crate::concurrent!(&mut producer, &mut consumer).expect("the producer and consumer complete");

    assert_eq!((producer.next, consumer.received), (ITEMS, ITEMS));
    assert!(producer.pending.is_none() && consumer.back.is_none());
    assert_eq!(
        producer.spares.len(),
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
