// What the primitives allocate once they are running, counted rather than asserted in prose.
//
// The claim under test is narrow and is the one the hot path depends on: acquiring an
// uncontended turn, drafting and publishing a version, and handing a payload over allocate
// nothing. Storage is taken at construction and reused. The counter is per thread, so tests
// running beside this one do not disturb it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use crate::cpu::sync::handoff::stocked;
use crate::cpu::sync::publish::{SLOTS, published};
use crate::cpu::sync::turn::Exclusive;

thread_local! {
    /// Allocations this thread has made. `Cell<usize>` needs no destructor, so registering it
    /// cannot itself allocate.
    static COUNTED: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call forwards to the system allocator with the same arguments, and the counter
// is a thread-local `Cell` that allocates nothing, so the allocator's own contract is untouched.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNTED.try_with(|counted| counted.set(counted.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Allocations made by this thread while `during` ran.
fn allocations(during: impl FnOnce()) -> usize {
    let before = COUNTED.with(Cell::get);
    during();
    COUNTED.with(Cell::get) - before
}

#[test]
fn an_uncontended_turn_allocates_nothing() {
    let value = Exclusive::new(0u64);
    *value.lock() += 1;
    let counted = allocations(|| {
        for _ in 0..100 {
            *value.lock() += 1;
        }
        assert!(value.try_lock().is_some());
    });
    assert_eq!(counted, 0);
    assert_eq!(value.into_inner(), 101);
}

#[test]
fn publishing_and_taking_a_version_allocates_nothing() {
    let (mut writer, mut reader) = published(SLOTS, || [0u64; 8]);
    let counted = allocations(|| {
        for round in 1..=100u64 {
            writer.draft()[0] = round;
            // One thread, so neither the buffers nor the metadata can be contended here.
            writer
                .publish()
                .expect("nothing is contended on one thread");
            let seen = reader.latest().expect("a version");
            assert_eq!(seen[0], round);
        }
    });
    assert_eq!(counted, 0);
}

#[test]
fn handing_a_reserved_block_over_allocates_nothing() {
    let (mut sender, mut receiver) = stocked(4, || Vec::<u8>::with_capacity(64));
    let counted = allocations(|| {
        for round in 0..100u8 {
            let mut block = sender.spare().expect("a reserved block");
            block.push(round);
            sender.send(block).expect("room for one payload");
            let mut back = receiver.recv().expect("the payload");
            assert_eq!(back.as_slice(), [round]);
            back.clear();
            // The refusal paths return the payload by value rather than allocating anything.
            receiver.give(back).expect("the pool had room");
        }
    });
    assert_eq!(counted, 0);
}
