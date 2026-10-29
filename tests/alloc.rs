//! No allocation per call and per frame. A global allocator counts the allocations of each thread,
//! so the test harness's own threads do not disturb the counts. Needs `--cfg trame_checked`, as
//! `parallel!` does.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::thread;
use trame::{Context, LEADER, Outcome, Topology};

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
fn parallel_allocates_on_the_first_call_of_a_size_and_never_after() {
    let items: Vec<Work> = (0..64).map(|i| Work { key: i % 4, value: u64::from(i) }).collect();
    let mut slots = [0_u64; 4];
    let before_first = count();
    trame::parallel!(add, &Ctx, &items, &mut slots);
    assert!(count() > before_first, "the first call sizes its buffers, so it allocates (the counter works)");

    let before = count();
    for _ in 0..1000 {
        trame::parallel!(add, &Ctx, &items, &mut slots);
    }
    assert_eq!(count() - before, 0, "1000 calls of the same size allocated");
}

/// A frame of plain data, sent between a worker and itself.
#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Tick(u32);

/// What a worker measured: allocations in its send-and-receive loop and in its barrier loop.
#[trame::message]
#[derive(Debug, PartialEq, Eq)]
struct Report {
    frames: u64,
    barriers: u64,
    /// One deliberate allocation: proves the counter sees this thread's allocations.
    control: u64,
}

const N: u32 = 1000;

fn measure(cx: &Context) {
    let me = cx.me();
    let before = count();
    for i in 0..N {
        trame::send(me, Tick(i)).expect("the route is lossy, so it is never full");
        assert_eq!(trame::recv::<Tick>(), Some(Tick(i)));
    }
    let frames = count() - before;

    let before = count();
    for _ in 0..N {
        let _publication = trame::barrier();
    }
    let barriers = count() - before;
    let before = count();
    std::hint::black_box(Box::new(7_u8));
    let control = count() - before;
    while trame::send(LEADER, Report { frames, barriers, control }).is_err() {}
}

#[test]
fn a_frame_and_a_barrier_allocate_nothing_in_a_worker() {
    // A worker thread is a fresh thread: its counter starts at zero, and only its own work counts.
    let handle = thread::spawn(|| {
        let mut g = Topology::new();
        let w = g.worker(measure);
        g.lossy::<Tick>(w, w, 4);
        g.route::<Report>(w, LEADER, 1);
        g.publish(b"init");
        let run = g.launch().expect("accepted");
        // A worker that ends without its report leaves the leader stuck, and seq panics the wait.
        let report = loop {
            if let Some(r) = run.recv::<Report>() {
                break r;
            }
            run.wait();
        };
        assert_eq!(run.join(), [Outcome::Returned]);
        report
    });
    let report = handle.join().expect("the test thread did not panic");
    assert!(report.control >= 1, "the control allocation was counted: {report:?}");
    assert_eq!((report.frames, report.barriers), (0, 0), "allocations in the worker: frames, barriers");
}
