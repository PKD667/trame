// What the handles may and may not do across threads.
//
// These are compile-time properties, so most of the file is a type check rather than a run. The
// negative ones — "this is deliberately not `Sync`" — cannot be written as a bound, so they use
// the usual autoref trick: an inherent item that exists only when the bound holds shadows a
// trait item that always exists, and which one answers says whether the bound held.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::cpu::sync::handoff::{Receiver, Sender};
use crate::cpu::sync::publish::{Pinned, Reader, Writer};
use crate::cpu::sync::turn::{Exclusive, Turn};
use crate::cpu::sync::{Cancel, Cancelled};

fn sendable<T: Send>() {}
fn sharable<T: Sync>() {}

/// `Probe::<T>::sync()` and `Probe::<T>::send()` are `true` only when the bound holds.
struct Probe<T>(PhantomData<T>);

trait Fallback {
    fn sync() -> bool {
        false
    }

    fn send() -> bool {
        false
    }
}

impl<T> Fallback for Probe<T> {}

impl<T: Sync> Probe<T> {
    fn sync() -> bool {
        true
    }
}

impl<T: Send> Probe<T> {
    fn send() -> bool {
        true
    }
}

#[test]
fn an_exclusive_resource_is_shared_only_when_its_value_can_move() {
    sendable::<Exclusive<u64>>();
    sharable::<Exclusive<u64>>();
    sendable::<Turn<'static, u64>>();
    sharable::<Turn<'static, u64>>();
    // A value that cannot leave its thread cannot be shared through the resource either.
    assert!(!Probe::<Exclusive<Rc<u64>>>::sync());
    // The guard hands out `&mut T`, so sharing one needs `T: Sync`, not `T: Send`.
    assert!(!Probe::<Turn<'static, Cell<u64>>>::sync());
    assert!(Probe::<Turn<'static, u64>>::sync());
}

#[test]
fn a_cancel_token_is_shared_by_everyone_it_stops() {
    sendable::<Cancel>();
    sharable::<Cancel>();
    sendable::<Cancelled>();
}

#[test]
fn publication_handles_move_to_their_thread_and_are_not_shared() {
    sendable::<Writer<Vec<u64>>>();
    sendable::<Reader<Vec<u64>>>();
    // One writer, one reader: a handle belongs to one thread at a time, so neither is `Sync`.
    assert!(!Probe::<Writer<Vec<u64>>>::sync());
    assert!(!Probe::<Reader<Vec<u64>>>::sync());
    // A pinned version is an ordinary shared reference to the reader's own buffer.
    sharable::<Pinned<'static, u64>>();
    assert!(!Probe::<Pinned<'static, Cell<u64>>>::sync());
}

#[test]
fn handoff_handles_move_to_their_thread_and_are_not_shared() {
    sendable::<Sender<Vec<u8>>>();
    sendable::<Receiver<Vec<u8>>>();
    assert!(!Probe::<Sender<Vec<u8>>>::sync());
    assert!(!Probe::<Receiver<Vec<u8>>>::sync());
}

#[test]
fn a_payload_that_cannot_leave_its_thread_cannot_be_handed_over() {
    // The handle for an `Rc` payload exists, but it cannot be moved to the thread that would
    // receive from it, which is what keeps a non-`Send` payload on the thread that made it.
    assert!(!Probe::<Sender<Rc<u64>>>::send());
    assert!(!Probe::<Receiver<Rc<u64>>>::send());
    assert!(Probe::<Sender<Vec<u8>>>::send());
    // Same for a publication: the buffers move between the two handles.
    assert!(!Probe::<Writer<Rc<u64>>>::send());
    assert!(!Probe::<Reader<Rc<u64>>>::send());
    assert!(Probe::<Writer<Vec<u64>>>::send());
}
