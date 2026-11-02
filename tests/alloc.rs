//! No allocation per call and per frame. A global allocator counts the allocations of each thread,
//! so the test harness's own threads do not disturb the counts. Needs `--cfg trame_checked`, as
//! `parallel!` does.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::thread;
use trame::{Outcome, Topology};

struct Counting;

thread_local! {
    /// Allocations made by this thread. A `Cell` of a `Copy` type needs no allocation to access.
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

fn count() -> u64 {
    ALLOCS.with(Cell::get)
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[derive(Clone, Copy)]
struct Work {
    key: u32,
    value: u64,
}

struct Ctx;

#[trame::item(key = x.key as usize)]
fn add(x: &Work, slot: &mut u64, _cx: &Ctx) {
    *slot = slot.wrapping_add(x.value);
}

#[test]
fn parallel_never_allocates() {
    let before_control = count();
    drop(std::hint::black_box(Box::new(7_u64)));
    assert!(count() > before_control, "the control allocation was counted (the counter works)");

    for n in [0_u32, 1, 64, 1000] {
        let items: Vec<Work> = (0..n).map(|i| Work { key: i % 4, value: u64::from(i) }).collect();
        let mut slots = [0_u64; 4];
        let at = count();
        for _ in 0..100 {
            trame::parallel!(add, &Ctx, &items, &mut slots);
        }
        assert_eq!(count() - at, 0, "100 calls over {n} items allocated, the first included");
    }
}

/// A frame of plain data, sent between a worker and itself.
#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Tick(u32);

const N: u32 = 1000;

/// One step sends a frame to its own worker and takes it back, N times. It reads this thread's
/// allocation count at its first and at its last run, so the difference covers N - 1 whole rounds of
/// step and scheduler (D5: a step, and the scheduler between steps, allocate nothing).
#[trame::state(steps(tick))]
struct Ticker {
    i: u32,
    first: u64,
    last: u64,
    /// One deliberate allocation: proves the counter sees this thread's allocations.
    control: u64,
}

#[trame::step(Ticker, receives = Tick)]
fn tick(i: &mut u32, first: &mut u64, last: &mut u64, control: &mut u64) -> trame::Status {
    if *i == 0 {
        *first = count();
    }
    trame::send(trame::me(), Tick(*i)).expect("the route is lossy, so it is never full");
    assert_eq!(trame::recv::<Tick>(), Some(Tick(*i)));
    *i += 1;
    if *i < N {
        return trame::Status::Progress;
    }
    *last = count();
    let before = count();
    std::hint::black_box(Box::new(7_u8));
    *control = count() - before;
    trame::Status::Done
}

#[test]
fn steps_and_the_scheduler_allocate_nothing_per_step() {
    // The leader runs on a fresh thread so its own counter is not the harness's.
    let handle = thread::spawn(|| {
        let mut g = Topology::new();
        // One agent: the counts are per thread, so every step must run on the same one.
        g.__agents(1);
        let w = g.worker();
        g.set(&w, move || Ticker { i: 0, first: 0, last: 0, control: 0 });
        g.lossy::<Tick>(w.id(), w.id(), 4);
        let run = g.launch().expect("accepted");
        assert_eq!(run.join(), [Outcome::Returned]);
        w.take()
    });
    let t = handle.join().expect("the test thread did not panic");
    assert!(t.control >= 1, "the control allocation was counted");
    assert_eq!(t.last - t.first, 0, "allocations in {} rounds of step and scheduler", N - 1);
}
