//! Family B over host memory prepared exactly as a launch prepares device memory.

use core::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::nv::sync::{Cancel, Ended, Idle, Pressure, Refused};
use crate::nv::sync::{handoff, publish};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pair {
    left: u64,
    right: u64,
}

#[test]
fn publication_moves_whole_slots_and_keeps_a_pin_stable() {
    let mut words = vec![0u32; publish::WORDS];
    publish::init_header(&mut words);
    let mut slots = vec![Pair { left: 0, right: 0 }; 3];
    let (mut writer, mut reader) =
        unsafe { publish::published(words.as_mut_ptr(), slots.as_mut_ptr(), slots.len() as u32) };

    assert!(reader.latest(8).expect("metadata is free").is_none());
    *writer.draft() = Pair { left: 1, right: 1 };
    let first = writer.publish().expect("three slots leave one spare");
    let pinned = reader
        .latest(8)
        .expect("metadata is free")
        .expect("the first version exists");
    assert_eq!(pinned.version(), first);
    assert_eq!(*pinned, Pair { left: 1, right: 1 });

    *writer.draft() = Pair { left: 2, right: 2 };
    writer.publish().expect("the third slot is free");
    *writer.draft() = Pair { left: 3, right: 3 };
    writer
        .publish()
        .expect("the untaken publication is recycled");

    // The writer moved on twice, but the slot behind this borrow still belongs to the reader.
    assert_eq!(*pinned, Pair { left: 1, right: 1 });
    assert_eq!(writer.overwritten(), 1);
    drop(pinned);

    let latest = reader
        .latest(8)
        .expect("metadata is free")
        .expect("a later version exists");
    assert_eq!(latest.version().count(), 3);
    assert_eq!(*latest, Pair { left: 3, right: 3 });
}

#[test]
fn two_publication_slots_report_capacity_without_touching_the_draft() {
    let mut words = vec![0u32; publish::WORDS];
    publish::init_header(&mut words);
    let mut slots = vec![0u64; 2];
    let (mut writer, mut reader) =
        unsafe { publish::published(words.as_mut_ptr(), slots.as_mut_ptr(), slots.len() as u32) };

    *writer.draft() = 7;
    writer.publish().expect("the reader holds no slot yet");
    assert_eq!(*reader.latest(8).unwrap().unwrap(), 7);
    *writer.draft() = 11;
    assert_eq!(writer.publish(), Err(Pressure::Full));
    assert_eq!(*writer.draft(), 11, "a refusal keeps the complete draft");
    assert_eq!(writer.refused(), 1);
    assert_eq!(writer.version().count(), 1);
}

#[test]
fn publication_drains_the_last_version_then_reports_closed() {
    let mut words = vec![0u32; publish::WORDS];
    publish::init_header(&mut words);
    let mut slots = vec![0u64; 3];
    let (mut writer, mut reader) =
        unsafe { publish::published(words.as_mut_ptr(), slots.as_mut_ptr(), slots.len() as u32) };
    *writer.draft() = 19;
    let version = writer.publish().unwrap();
    drop(writer);

    let cancel = Cancel::default();
    assert_eq!(*reader.after(Default::default(), 32, &cancel).unwrap(), 19);
    assert_eq!(
        reader.after(version, 32, &cancel).err(),
        Some(Ended::Closed)
    );
}

fn empty_slots<T>(depth: usize) -> Vec<MaybeUninit<T>> {
    std::iter::repeat_with(MaybeUninit::uninit)
        .take(depth)
        .collect()
}

#[test]
fn handoff_is_fifo_and_returns_every_refused_payload() {
    let depth = 2usize;
    let mut words = vec![0u32; handoff::WORDS];
    handoff::init_header(&mut words);
    let mut full = empty_slots::<String>(depth);
    let mut free = empty_slots::<String>(depth);
    let (mut sender, mut receiver) = unsafe {
        handoff::handoff(
            words.as_mut_ptr(),
            full.as_mut_ptr(),
            free.as_mut_ptr(),
            depth as u32,
        )
    };

    sender.send("one".to_owned()).unwrap();
    sender.send("two".to_owned()).unwrap();
    let unsent = sender.send("three".to_owned()).unwrap_err();
    assert_eq!(unsent.why, Refused::Full);
    assert_eq!(unsent.into_inner(), "three");
    assert_eq!(receiver.recv().unwrap(), "one");
    assert_eq!(receiver.recv().unwrap(), "two");
    assert_eq!(receiver.recv(), Err(Idle::Empty));

    drop(sender);
    assert_eq!(receiver.recv(), Err(Idle::Closed));
}

#[test]
fn stocked_storage_makes_the_round_trip_without_allocation() {
    let depth = 3usize;
    let mut words = vec![0u32; handoff::WORDS];
    handoff::init_header(&mut words);
    let mut full = empty_slots::<Vec<u8>>(depth);
    let mut free: Vec<MaybeUninit<Vec<u8>>> = (0..depth)
        .map(|_| MaybeUninit::new(Vec::with_capacity(16)))
        .collect();
    let (mut sender, mut receiver) = unsafe {
        handoff::stocked(
            words.as_mut_ptr(),
            full.as_mut_ptr(),
            free.as_mut_ptr(),
            depth as u32,
        )
    };

    let mut block = sender.spare().expect("the launch stocked every spare");
    block.extend_from_slice(b"payload");
    sender.send(block).unwrap();
    let mut arrived = receiver.recv().unwrap();
    assert_eq!(arrived, b"payload");
    arrived.clear();
    receiver.give(arrived).unwrap();
    assert_eq!(sender.spare().unwrap(), Vec::<u8>::new());
}

#[test]
fn a_contended_publication_reports_busy_and_keeps_its_draft() {
    let mut words = vec![0u32; publish::WORDS];
    publish::init_header(&mut words);
    let mut slots = vec![0u64; 3];
    let (mut writer, reader) =
        unsafe { publish::published(words.as_mut_ptr(), slots.as_mut_ptr(), slots.len() as u32) };

    let held = reader.hold();
    *writer.draft() = 5;
    assert_eq!(writer.publish(), Err(Pressure::Busy));
    assert_eq!(
        *writer.draft(),
        5,
        "a contended publication keeps the draft"
    );
    assert_eq!(writer.busy(), 1, "busy is counted apart from capacity");
    assert_eq!(writer.refused(), 0);
    assert_eq!(writer.version().count(), 0, "no version was committed");
    drop(held);
    assert!(writer.publish().is_ok(), "the next attempt succeeds");
}

#[test]
fn a_cancelled_or_exhausted_wait_consumes_no_version() {
    let mut words = vec![0u32; publish::WORDS];
    publish::init_header(&mut words);
    let mut slots = vec![0u64; 3];
    let (mut writer, mut reader) =
        unsafe { publish::published(words.as_mut_ptr(), slots.as_mut_ptr(), slots.len() as u32) };
    *writer.draft() = 1;
    writer.publish().unwrap();
    let seen = reader.latest(8).unwrap().unwrap().version();

    // The writer exists, so nothing is `Closed`; both endings come from the wait itself.
    let cancel = Cancel::default();
    cancel.raise();
    assert_eq!(reader.after(seen, 8, &cancel).err(), Some(Ended::Cancelled));
    assert_eq!(
        reader.after(seen, 8, &Cancel::default()).err(),
        Some(Ended::Exhausted { attempts: 8 }),
        "an unbounded number of attempts on a stalled writer still ends"
    );
    assert_eq!(
        reader.version(),
        seen,
        "no wait consumed a version it did not take"
    );

    // And the reader can still take the next real version afterwards.
    *writer.draft() = 2;
    writer.publish().unwrap();
    assert_eq!(*reader.latest(8).unwrap().unwrap(), 2);
}

#[test]
fn a_contended_handoff_send_keeps_its_payload_and_reports_busy() {
    let depth = 2usize;
    let mut words = vec![0u32; handoff::WORDS];
    handoff::init_header(&mut words);
    let mut full = empty_slots::<u32>(depth);
    let mut free = empty_slots::<u32>(depth);
    let (mut sender, receiver) = unsafe {
        handoff::handoff(
            words.as_mut_ptr(),
            full.as_mut_ptr(),
            free.as_mut_ptr(),
            depth as u32,
        )
    };

    let held = receiver.hold();
    let unsent = sender.send(9).unwrap_err();
    assert_eq!(unsent.why, Refused::Busy, "contention is not capacity");
    assert_eq!(unsent.into_inner(), 9, "the payload comes straight back");
    drop(held);
    // `spares` waits for the metadata lock, so it is asked only once this thread holds nothing.
    assert_eq!(receiver.spares(), 0);
    sender.send(9).expect("the next attempt has the lock");
}

#[derive(Debug)]
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn dropping_endpoints_drops_each_owned_payload_once() {
    let depth = 2usize;
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut words = vec![0u32; handoff::WORDS];
    handoff::init_header(&mut words);
    let mut full = empty_slots::<Counted>(depth);
    let mut free = empty_slots::<Counted>(depth);
    let (mut sender, receiver) = unsafe {
        handoff::handoff(
            words.as_mut_ptr(),
            full.as_mut_ptr(),
            free.as_mut_ptr(),
            depth as u32,
        )
    };
    sender.send(Counted(Arc::clone(&dropped))).unwrap();
    sender.send(Counted(Arc::clone(&dropped))).unwrap();
    drop(receiver);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);

    let refused = sender.send(Counted(Arc::clone(&dropped))).unwrap_err();
    assert_eq!(refused.why, Refused::Closed);
    drop(refused);
    assert_eq!(dropped.load(Ordering::SeqCst), 3);
}
